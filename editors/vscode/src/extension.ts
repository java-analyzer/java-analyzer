import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

function createClient(): LanguageClient {
  const config = vscode.workspace.getConfiguration("javaAnalyzer");
  const command = config.get<string>("server.path", "java-analyzer");
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

async function startClient(): Promise<void> {
  client = createClient();
  try {
    await client.start();
  } catch (err) {
    client = undefined;
    vscode.window.showErrorMessage(
      `Failed to start java-analyzer: ${err}. Set "javaAnalyzer.server.path" or put the binary on PATH.`,
    );
  }
}

async function stopClient(): Promise<void> {
  const current = client;
  client = undefined;
  await current?.stop();
}

export async function activate(
  context: vscode.ExtensionContext,
): Promise<void> {
  context.subscriptions.push(
    vscode.commands.registerCommand("javaAnalyzer.restartServer", async () => {
      await stopClient();
      await startClient();
    }),
  );
  await startClient();
}

export async function deactivate(): Promise<void> {
  await stopClient();
}
