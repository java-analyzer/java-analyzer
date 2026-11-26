import { execFile } from "node:child_process";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { promisify } from "node:util";
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";
import {
  ArchiveExt,
  BuildInfo,
  InstalledServerMeta,
  PlatformTarget,
  UpdateChannel,
  buildAssetName,
  buildExtractedBinaryRelativePath,
  isUpdateNeeded,
  parseBuildInfo,
  parseSha256Sums,
  releaseApiUrl,
  resolvePlatformTarget,
  shouldCheckForUpdates,
  verifyFileSha256,
} from "./release";

const execFileAsync = promisify(execFile);

const FALLBACK_BINARY = "java-analyzer";
const UPDATE_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;
const LAST_UPDATE_CHECK_KEY = "javaAnalyzer.lastUpdateCheckMs";
const USER_AGENT = "java-analyzer-vscode";

interface GithubReleaseAsset {
  name: string;
  browser_download_url: string;
}

interface GithubReleaseResponse {
  tag_name: string;
  assets: GithubReleaseAsset[];
}

interface RemoteReleasePlan {
  tag: string;
  archiveUrl: string;
  assetName: string;
  sha256SumsUrl: string;
  buildInfoUrl?: string;
  target: PlatformTarget;
}

let client: LanguageClient | undefined;
let outputChannel: vscode.OutputChannel | undefined;
let activeUpdatePromise: Promise<InstalledServerMeta | undefined> | undefined;

function getOutputChannel(): vscode.OutputChannel {
  if (!outputChannel) {
    outputChannel = vscode.window.createOutputChannel("Java Analyzer Extension");
  }
  return outputChannel;
}

function logInfo(message: string): void {
  getOutputChannel().appendLine(`[info] ${message}`);
}

function logWarn(message: string): void {
  getOutputChannel().appendLine(`[warn] ${message}`);
}

function getServerRoot(context: vscode.ExtensionContext): string {
  return path.join(context.globalStorageUri.fsPath, "server");
}

function getCurrentJsonPath(serverRoot: string): string {
  return path.join(serverRoot, "current.json");
}

async function fileExists(filePath: string): Promise<boolean> {
  try {
    const stat = await fs.stat(filePath);
    return stat.isFile();
  } catch {
    return false;
  }
}

async function readInstalledMeta(
  serverRoot: string,
): Promise<InstalledServerMeta | undefined> {
  try {
    const raw = await fs.readFile(getCurrentJsonPath(serverRoot), "utf8");
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) {
      return undefined;
    }
    const record = parsed as Record<string, unknown>;
    if (
      typeof record.tag !== "string" ||
      typeof record.binaryPath !== "string"
    ) {
      return undefined;
    }
    if (!(await fileExists(record.binaryPath))) {
      return undefined;
    }
    return {
      tag: record.tag,
      commit: typeof record.commit === "string" ? record.commit : undefined,
      builtAt: typeof record.builtAt === "string" ? record.builtAt : undefined,
      binaryPath: record.binaryPath,
    };
  } catch {
    return undefined;
  }
}

async function writeInstalledMeta(
  serverRoot: string,
  meta: InstalledServerMeta,
): Promise<void> {
  await fs.mkdir(serverRoot, { recursive: true });
  const targetPath = getCurrentJsonPath(serverRoot);
  const tmpPath = `${targetPath}.tmp`;
  await fs.writeFile(tmpPath, `${JSON.stringify(meta, null, 2)}\n`, "utf8");
  await fs.rename(tmpPath, targetPath);
}

async function pruneOldServerDirs(
  serverRoot: string,
  keepTag: string,
): Promise<void> {
  try {
    const entries = await fs.readdir(serverRoot, { withFileTypes: true });
    for (const entry of entries) {
      if (!entry.isDirectory()) {
        continue;
      }
      if (entry.name === keepTag) {
        continue;
      }
      await fs.rm(path.join(serverRoot, entry.name), {
        recursive: true,
        force: true,
      });
    }
  } catch (err) {
    logWarn(`Failed to prune old server directories: ${err}`);
  }
}

async function fetchJson<T>(url: string): Promise<T> {
  const response = await fetch(url, {
    headers: {
      Accept: "application/vnd.github+json",
      "User-Agent": USER_AGENT,
    },
  });
  if (!response.ok) {
    throw new Error(
      `HTTP ${response.status} ${response.statusText} fetching ${url}`,
    );
  }
  return (await response.json()) as T;
}

async function fetchText(url: string): Promise<string> {
  const response = await fetch(url, {
    headers: {
      "User-Agent": USER_AGENT,
    },
  });
  if (!response.ok) {
    throw new Error(
      `HTTP ${response.status} ${response.statusText} fetching ${url}`,
    );
  }
  return response.text();
}

async function downloadToFile(url: string, destPath: string): Promise<void> {
  const response = await fetch(url, {
    headers: {
      "User-Agent": USER_AGENT,
    },
  });
  if (!response.ok) {
    throw new Error(
      `HTTP ${response.status} ${response.statusText} downloading ${url}`,
    );
  }
  const bytes = Buffer.from(await response.arrayBuffer());
  await fs.writeFile(destPath, bytes);
}

async function extractArchive(
  archivePath: string,
  destDir: string,
  archiveExt: ArchiveExt,
): Promise<void> {
  if (archiveExt === "tar.gz") {
    await execFileAsync("tar", ["-xzf", archivePath, "-C", destDir]);
    return;
  }
  try {
    await execFileAsync("tar", ["-xf", archivePath, "-C", destDir]);
  } catch {
    await execFileAsync("powershell.exe", [
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      `Expand-Archive -LiteralPath '${archivePath.replace(/'/g, "''")}' -DestinationPath '${destDir.replace(/'/g, "''")}' -Force`,
    ]);
  }
}

async function fetchRemoteReleasePlan(
  channel: Exclude<UpdateChannel, "off">,
): Promise<RemoteReleasePlan> {
  const target = resolvePlatformTarget(process.platform, process.arch);
  if (!target) {
    throw new Error(
      `Unsupported platform ${process.platform}/${process.arch} for prebuilt java-analyzer binaries`,
    );
  }

  const release = await fetchJson<GithubReleaseResponse>(
    releaseApiUrl(channel),
  );
  const tag = release.tag_name;
  const assetName = buildAssetName(tag, target.triple, target.archiveExt);
  const archiveAsset = release.assets.find((a) => a.name === assetName);
  if (!archiveAsset) {
    throw new Error(
      `Release ${tag} does not contain asset ${assetName}`,
    );
  }
  const shaAsset = release.assets.find((a) => a.name === "SHA256SUMS");
  if (!shaAsset) {
    throw new Error(`Release ${tag} does not contain SHA256SUMS`);
  }
  const buildInfoAsset = release.assets.find(
    (a) => a.name === "BUILD_INFO.json",
  );

  return {
    tag,
    archiveUrl: archiveAsset.browser_download_url,
    assetName,
    sha256SumsUrl: shaAsset.browser_download_url,
    buildInfoUrl: buildInfoAsset?.browser_download_url,
    target,
  };
}

async function fetchRemoteBuildInfo(
  plan: RemoteReleasePlan,
): Promise<BuildInfo | undefined> {
  if (!plan.buildInfoUrl) {
    return undefined;
  }
  try {
    const raw = await fetchText(plan.buildInfoUrl);
    return parseBuildInfo(raw);
  } catch (err) {
    logWarn(`Failed to fetch BUILD_INFO.json for ${plan.tag}: ${err}`);
    return undefined;
  }
}

async function downloadAndInstallRelease(
  serverRoot: string,
  plan: RemoteReleasePlan,
  buildInfo: BuildInfo | undefined,
): Promise<InstalledServerMeta> {
  await fs.mkdir(serverRoot, { recursive: true });
  const archiveTmpPath = path.join(
    serverRoot,
    `${plan.assetName}.${Date.now()}.download`,
  );
  const stagingDir = path.join(
    serverRoot,
    `${plan.tag}.${Date.now()}.staging`,
  );
  const finalTagDir = path.join(serverRoot, plan.tag);

  try {
    const shaSumsText = await fetchText(plan.sha256SumsUrl);
    const expectedSha = parseSha256Sums(shaSumsText, plan.assetName);
    if (!expectedSha) {
      throw new Error(
        `SHA256SUMS does not contain checksum for ${plan.assetName}`,
      );
    }

    await downloadToFile(plan.archiveUrl, archiveTmpPath);

    const valid = await verifyFileSha256(archiveTmpPath, expectedSha);
    if (!valid) {
      throw new Error(
        `SHA-256 verification failed for ${plan.assetName}`,
      );
    }

    await fs.mkdir(stagingDir, { recursive: true });
    await extractArchive(archiveTmpPath, stagingDir, plan.target.archiveExt);

    const relativeBin = buildExtractedBinaryRelativePath(
      plan.tag,
      plan.target.triple,
      plan.target.binName,
    );
    const stagedBinPath = path.join(stagingDir, relativeBin);
    if (!(await fileExists(stagedBinPath))) {
      throw new Error(
        `Extracted archive missing expected binary at ${relativeBin}`,
      );
    }

    if (process.platform !== "win32") {
      await fs.chmod(stagedBinPath, 0o755);
    }

    await fs.rm(finalTagDir, { recursive: true, force: true });
    await fs.rename(stagingDir, finalTagDir);

    const installedBinPath = path.join(finalTagDir, relativeBin);
    const meta: InstalledServerMeta = {
      tag: plan.tag,
      commit: buildInfo?.commit,
      builtAt: buildInfo?.built_at,
      binaryPath: installedBinPath,
    };

    await writeInstalledMeta(serverRoot, meta);
    await pruneOldServerDirs(serverRoot, plan.tag);
    return meta;
  } finally {
    await fs.rm(archiveTmpPath, { force: true }).catch(() => {});
    await fs.rm(stagingDir, { recursive: true, force: true }).catch(() => {});
  }
}

function getExplicitServerPath(): string {
  const config = vscode.workspace.getConfiguration("javaAnalyzer");
  return config.get<string>("server.path", "").trim();
}

function getUpdateChannel(): UpdateChannel {
  const config = vscode.workspace.getConfiguration("javaAnalyzer");
  return config.get<UpdateChannel>("updates.channel", "stable");
}

function createClient(command: string): LanguageClient {
  const config = vscode.workspace.getConfiguration("javaAnalyzer");
  const args = config.get<string[]>("server.args", []);

  const serverOptions: ServerOptions = {
    command,
    args,
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "java" }],
  };

  return new LanguageClient(
    "javaAnalyzer",
    "Java Analyzer",
    serverOptions,
    clientOptions,
  );
}

async function startClient(command: string): Promise<void> {
  client = createClient(command);
  try {
    await client.start();
  } catch (err) {
    client = undefined;
    vscode.window.showErrorMessage(
      `Failed to start java-analyzer (${command}): ${err}. Set "javaAnalyzer.server.path" or put the binary on PATH.`,
    );
  }
}

async function stopClient(): Promise<void> {
  const current = client;
  client = undefined;
  await current?.stop();
}

async function installInitialServerWithProgress(
  context: vscode.ExtensionContext,
  channel: Exclude<UpdateChannel, "off">,
): Promise<InstalledServerMeta | undefined> {
  const serverRoot = getServerRoot(context);
  return vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      title: "Downloading java-analyzer language server...",
      cancellable: false,
    },
    async (progress) => {
      try {
        progress.report({ message: `Checking ${channel} release` });
        const plan = await fetchRemoteReleasePlan(channel);
        const buildInfo = await fetchRemoteBuildInfo(plan);
        progress.report({ message: `Installing ${plan.tag}` });
        const meta = await downloadAndInstallRelease(
          serverRoot,
          plan,
          buildInfo,
        );
        await context.globalState.update(LAST_UPDATE_CHECK_KEY, Date.now());
        logInfo(`Installed java-analyzer ${meta.tag} at ${meta.binaryPath}`);
        return meta;
      } catch (err) {
        logWarn(
          `Automatic java-analyzer download failed (${err}); falling back to PATH.`,
        );
        return undefined;
      }
    },
  );
}

async function checkAndApplyUpdate(
  context: vscode.ExtensionContext,
  options: { manual: boolean },
): Promise<void> {
  if (activeUpdatePromise) {
    await activeUpdatePromise;
    return;
  }

  const explicit = getExplicitServerPath();
  if (explicit.length > 0) {
    if (options.manual) {
      void vscode.window.showInformationMessage(
        `java-analyzer is using explicit server path "${explicit}"; automatic updates are skipped.`,
      );
    }
    return;
  }

  const channel = getUpdateChannel();
  if (channel === "off") {
    if (options.manual) {
      void vscode.window.showInformationMessage(
        'java-analyzer automatic updates are disabled ("javaAnalyzer.updates.channel": "off").',
      );
    }
    return;
  }

  const serverRoot = getServerRoot(context);
  const run = async (): Promise<InstalledServerMeta | undefined> => {
    const installed = await readInstalledMeta(serverRoot);
    const plan = await fetchRemoteReleasePlan(channel);
    const buildInfo =
      plan.tag === "nightly" || !installed || installed.tag !== plan.tag
        ? await fetchRemoteBuildInfo(plan)
        : undefined;

    await context.globalState.update(LAST_UPDATE_CHECK_KEY, Date.now());

    if (!isUpdateNeeded(installed, plan.tag, buildInfo?.commit)) {
      logInfo(`java-analyzer ${plan.tag} is up to date.`);
      if (options.manual) {
        void vscode.window.showInformationMessage(
          `java-analyzer is up to date (${plan.tag}).`,
        );
      }
      return undefined;
    }

    const updated = await downloadAndInstallRelease(serverRoot, plan, buildInfo);
    logInfo(`Updated java-analyzer to ${updated.tag}`);
    return updated;
  };

  activeUpdatePromise = (async () => {
    try {
      const updated = options.manual
        ? await vscode.window.withProgress(
            {
              location: vscode.ProgressLocation.Notification,
              title: "Checking for java-analyzer updates...",
              cancellable: false,
            },
            run,
          )
        : await run();

      if (updated) {
        await stopClient();
        await startClient(updated.binaryPath);
        void vscode.window.showInformationMessage(
          `Updated java-analyzer to ${updated.tag}.`,
        );
      }
      return updated;
    } catch (err) {
      const msg = `Failed to check for java-analyzer updates: ${err}`;
      logWarn(msg);
      if (options.manual) {
        void vscode.window.showWarningMessage(msg);
      }
      return undefined;
    } finally {
      activeUpdatePromise = undefined;
    }
  })();

  await activeUpdatePromise;
}

async function resolveCommandForStart(
  context: vscode.ExtensionContext,
): Promise<{ command: string; shouldCheckBackgroundUpdate: boolean }> {
  const explicit = getExplicitServerPath();
  if (explicit.length > 0) {
    return { command: explicit, shouldCheckBackgroundUpdate: false };
  }

  const serverRoot = getServerRoot(context);
  const channel = getUpdateChannel();
  const installed = await readInstalledMeta(serverRoot);
  if (installed) {
    return {
      command: installed.binaryPath,
      shouldCheckBackgroundUpdate: channel !== "off",
    };
  }

  if (channel !== "off") {
    const downloaded = await installInitialServerWithProgress(context, channel);
    if (downloaded) {
      return {
        command: downloaded.binaryPath,
        shouldCheckBackgroundUpdate: false,
      };
    }
  }

  return { command: FALLBACK_BINARY, shouldCheckBackgroundUpdate: false };
}

export async function activate(
  context: vscode.ExtensionContext,
): Promise<void> {
  context.subscriptions.push(getOutputChannel());

  context.subscriptions.push(
    vscode.commands.registerCommand("javaAnalyzer.restartServer", async () => {
      await stopClient();
      const { command } = await resolveCommandForStart(context);
      await startClient(command);
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(
      "javaAnalyzer.checkForUpdates",
      async () => {
        await checkAndApplyUpdate(context, { manual: true });
      },
    ),
  );

  const { command, shouldCheckBackgroundUpdate } =
    await resolveCommandForStart(context);
  await startClient(command);

  if (shouldCheckBackgroundUpdate) {
    const lastChecked = context.globalState.get<number>(LAST_UPDATE_CHECK_KEY);
    if (
      shouldCheckForUpdates(
        lastChecked,
        Date.now(),
        UPDATE_CHECK_INTERVAL_MS,
      )
    ) {
      void checkAndApplyUpdate(context, { manual: false });
    }
  }
}

export async function deactivate(): Promise<void> {
  await stopClient();
}
