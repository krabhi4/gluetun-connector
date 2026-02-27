const Docker = require("dockerode");
const fs = require("fs");
const { execSync } = require("child_process");

const CHECK_INTERVAL = parseInt(process.env.CHECK_INTERVAL, 10) || 30;
const TIMEOUT = parseInt(process.env.TIMEOUT, 10) || 10;
const FAIL_THRESHOLD = parseInt(process.env.FAIL_THRESHOLD, 10) || 2;
const GLUETUN_CONTAINER = process.env.GLUETUN_CONTAINER || "gluetun";
const DEPENDENT_CONTAINERS = process.env.DEPENDENT_CONTAINERS || "auto";
const HEALTHY_WAIT_TIMEOUT =
  parseInt(process.env.HEALTHY_WAIT_TIMEOUT, 10) || 120;
const CONFIG_FILE = process.env.CONFIG_FILE || "/config/sites.conf";

const docker = new Docker({ socketPath: "/var/run/docker.sock" });

let isMonitoring = false;
let monitorInterval;
let siteFailures = {};

const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function getGluetunContainer() {
  const containers = await docker.listContainers({ all: true });
  return containers.find((c) => c.Names.includes(`/${GLUETUN_CONTAINER}`));
}

function loadConfiguredSites() {
  if (!fs.existsSync(CONFIG_FILE)) {
    console.error(`[MONITOR] Config file not found: ${CONFIG_FILE}`);
    return [];
  }
  const content = fs.readFileSync(CONFIG_FILE, "utf-8");
  return content
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith("#"));
}

async function testSiteAsync(containerId, site) {
  return new Promise((resolve) => {
    const startTime = Date.now();
    const cmd = [
      "wget",
      "--spider",
      "-S",
      `--timeout=${TIMEOUT}`,
      "--tries=1",
      "-q",
      site,
    ];

    docker
      .getContainer(containerId)
      .exec(
        { Cmd: cmd, AttachStdout: true, AttachStderr: true },
        (err, exec) => {
          if (err) {
            return resolve({
              site,
              status: "FAIL",
              duration: Date.now() - startTime,
              reason: "Failed to create exec instance",
            });
          }

          exec.start((err, stream) => {
            if (err) {
              return resolve({
                site,
                status: "FAIL",
                duration: Date.now() - startTime,
                reason: "Failed to start exec command",
              });
            }

            let output = "";
            stream.on("data", (chunk) => {
              output += chunk.toString("utf8");
            });

            stream.on("end", () => {
              exec.inspect((err, data) => {
                const exitCode = data ? data.ExitCode : -1;
                const duration = Date.now() - startTime;

                // Extract HTTP code (roughly)
                const httpMatch = output.match(/HTTP\/[0-9.]+ ([0-9]+)/);
                const httpCode = httpMatch ? httpMatch[1] : "N/A";

                // Decoding WGet exit code based on the bash script
                if (exitCode === 0) {
                  resolve({ site, status: "PASS", duration, httpCode });
                } else if (exitCode === 6 || exitCode === 8) {
                  resolve({
                    site,
                    status: "PASS",
                    duration,
                    reason: `HTTP ${httpCode} (VPN working)`,
                  });
                } else {
                  let reason = "Unknown error";
                  if (exitCode === 4)
                    reason = "Network failure (DNS or connection)";
                  if (exitCode === 5) reason = "SSL verification failure";
                  resolve({ site, status: "FAIL", duration, reason });
                }
              });
            });
          });
        },
      );
  });
}

async function discoverDependentContainers(gluetunId) {
  const shortId = gluetunId.substring(0, 12);
  const containers = await docker.listContainers();
  const dependents = [];

  for (const c of containers) {
    const name = c.Names[0].replace(/^\//, "");
    if (name === GLUETUN_CONTAINER) continue;

    const networkMode = c.HostConfig?.NetworkMode || "";
    if (
      networkMode === `container:${GLUETUN_CONTAINER}` ||
      networkMode === `container:${gluetunId}` ||
      networkMode.startsWith(`container:${shortId}`)
    ) {
      dependents.push(name);
    }
  }
  return dependents;
}

async function getDependentContainers(gluetunId) {
  if (DEPENDENT_CONTAINERS === "auto") {
    return await discoverDependentContainers(gluetunId);
  }
  return DEPENDENT_CONTAINERS.split(",")
    .map((s) => s.trim())
    .filter(Boolean);
}

async function waitForGluetunHealthy() {
  console.log(
    `[MONITOR] Waiting for ${GLUETUN_CONTAINER} to become healthy...`,
  );
  const maxWait = HEALTHY_WAIT_TIMEOUT;
  let waited = 0;

  while (waited < maxWait) {
    const containerInfo = await getGluetunContainer();
    if (containerInfo) {
      const gContainer = docker.getContainer(containerInfo.Id);
      const data = await gContainer.inspect();
      const status = data.State?.Health?.Status || "unknown";
      if (status === "healthy") {
        console.log(
          `[MONITOR] ${GLUETUN_CONTAINER} is healthy after ${waited}s`,
        );
        return true;
      }
    }
    await delay(5000);
    waited += 5;
  }
  console.error(
    `[MONITOR] ${GLUETUN_CONTAINER} did not become healthy within ${maxWait}s`,
  );
  return false;
}

async function restartGluetun() {
  console.log(
    `[MONITOR] Restarting ${GLUETUN_CONTAINER} to force new endpoint...`,
  );
  const containerInfo = await getGluetunContainer();
  if (!containerInfo) return false;

  const container = docker.getContainer(containerInfo.Id);
  try {
    await container.restart();
    const isHealthy = await waitForGluetunHealthy();
    if (!isHealthy) return false;

    // Wait for DNS
    console.log(`[MONITOR] Waiting for DNS to stabilize...`);
    await delay(10000); // 10 second conservative wait
    return true;
  } catch (e) {
    console.error(`[MONITOR] Failed to restart Gluetun:`, e);
    return false;
  }
}

async function restartDependentContainers(gluetunId) {
  console.log(`[MONITOR] Discovering and restarting dependent containers...`);
  const dependents = await getDependentContainers(gluetunId);
  if (dependents.length === 0) {
    console.warn(`[MONITOR] No dependent containers to restart`);
    return;
  }

  for (const name of dependents) {
    console.log(`[MONITOR] Restarting ${name}...`);
    try {
      // Search for target container
      const all = await docker.listContainers({ all: true });
      const target = all.find((c) => c.Names.includes(`/${name}`));
      if (target) {
        await docker.getContainer(target.Id).restart();
        console.log(`[MONITOR] ${name} restarted successfully`);
        await delay(2000); // pause between restarts
      } else {
        console.warn(`[MONITOR] Container ${name} not found, skipping`);
      }
    } catch (e) {
      console.error(`[MONITOR] Failed to restart ${name}:`, e.message);
    }
  }
  console.log(`[MONITOR] Dependent container restart complete`);
}

async function handleFailure(gluetunId) {
  console.warn(`[MONITOR] Health check failed, initiating recovery...`);
  const success = await restartGluetun();
  if (success) {
    // Re-verify connectivity here if needed
    console.log(`[MONITOR] Restarting dependent containers...`);
    await restartDependentContainers(gluetunId);

    // reset failures so we have a clean slate
    siteFailures = {};
    console.log(`[MONITOR] Recovery complete`);
  } else {
    console.error(
      `[MONITOR] Recovery failed - manual intervention may be required`,
    );
  }
}

async function performChecks() {
  const gluetunInfo = await getGluetunContainer();
  if (!gluetunInfo) {
    console.error(`[MONITOR] Error: Container ${GLUETUN_CONTAINER} not found!`);
    return;
  }

  const sites = loadConfiguredSites();
  if (sites.length === 0) return;

  const promises = sites.map((site) => testSiteAsync(gluetunInfo.Id, site));
  const results = await Promise.all(promises);

  let anyExceeded = false;
  results.forEach((r) => {
    if (r.status === "PASS") {
      siteFailures[r.site] = 0;
      console.log(`[MONITOR] Site ${r.site} PASS (${r.duration}ms)`);
    } else {
      siteFailures[r.site] = (siteFailures[r.site] || 0) + 1;
      const failures = siteFailures[r.site];
      if (failures >= FAIL_THRESHOLD) {
        anyExceeded = true;
        console.warn(
          `[MONITOR] Site ${r.site} FAILED ${failures} times (THRESHOLD REACHED) - ${r.reason}`,
        );
      } else {
        console.warn(
          `[MONITOR] Site ${r.site} FAILED (${failures}/${FAIL_THRESHOLD}) - ${r.reason}`,
        );
      }
    }
  });

  if (anyExceeded) {
    console.warn(`[MONITOR] Threshold reached! Initiating recovery...`);
    await handleFailure(gluetunInfo.Id);
  }
}

async function startMonitor() {
  if (isMonitoring) return;
  isMonitoring = true;
  console.log(
    `[MONITOR] Starting monitor for ${GLUETUN_CONTAINER} with interval ${CHECK_INTERVAL}s`,
  );

  if (DEPENDENT_CONTAINERS === "auto") {
    const info = await getGluetunContainer();
    if (info) {
      const deps = await discoverDependentContainers(info.Id);
      console.log(
        `[MONITOR] Initial dependent containers (auto-discovery): ${deps.join(", ") || "(none found)"}`,
      );
    }
  }

  monitorInterval = setInterval(async () => {
    await performChecks();
  }, CHECK_INTERVAL * 1000);
}

function stopMonitor() {
  if (monitorInterval) {
    clearInterval(monitorInterval);
  }
  isMonitoring = false;
  console.log(`[MONITOR] Stopped monitor`);
}

function getMonitorStatus() {
  return {
    isMonitoring,
    checkInterval: CHECK_INTERVAL,
    dependentContainers: DEPENDENT_CONTAINERS,
    siteFailures,
  };
}

module.exports = {
  startMonitor,
  stopMonitor,
  performChecks,
  getMonitorStatus,
};
