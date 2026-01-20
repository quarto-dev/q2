/**
 * Quarto Rust LSP Extension for VS Code
 *
 * This extension provides language support for Quarto documents (.qmd files)
 * by connecting to the Rust-based Quarto LSP server via stdio.
 */

import * as vscode from "vscode";
import * as path from "path";
import { execSync } from "child_process";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;
let outputChannel: vscode.OutputChannel;

/**
 * Find the path to the quarto binary.
 *
 * Priority:
 * 1. User-configured path in settings
 * 2. `quarto` in PATH
 *
 * @returns The path to the quarto binary, or undefined if not found
 */
function findQuartoPath(): string | undefined {
  const config = vscode.workspace.getConfiguration("quartoRustLsp");
  const configuredPath = config.get<string>("path");

  // Use configured path if provided
  if (configuredPath && configuredPath.trim() !== "") {
    return configuredPath;
  }

  // Try to find quarto in PATH
  try {
    const which = process.platform === "win32" ? "where" : "which";
    const result = execSync(`${which} quarto`, { encoding: "utf8" }).trim();
    // `which` may return multiple lines on Windows, take the first
    return result.split("\n")[0].trim();
  } catch {
    return undefined;
  }
}

/**
 * Activate the extension.
 *
 * This is called when VS Code loads the extension (based on activationEvents).
 */
export async function activate(
  context: vscode.ExtensionContext
): Promise<void> {
  outputChannel = vscode.window.createOutputChannel("Quarto Rust LSP");
  context.subscriptions.push(outputChannel);

  outputChannel.appendLine("Quarto Rust LSP extension activating...");

  // Find the quarto binary
  const quartoPath = findQuartoPath();
  if (!quartoPath) {
    const message =
      "Could not find `quarto` binary. Please install Quarto or set the `quartoRustLsp.path` setting.";
    vscode.window.showErrorMessage(message);
    outputChannel.appendLine(`Error: ${message}`);
    return;
  }

  outputChannel.appendLine(`Using quarto binary: ${quartoPath}`);

  // Configure the language server
  // Note: For Executable ServerOptions, stdio is the default transport.
  // We don't specify transport explicitly to avoid vscode-languageclient
  // potentially adding --stdio to the arguments.
  const serverOptions: ServerOptions = {
    command: quartoPath,
    args: ["lsp"],
    options: {
      // Run from workspace folder if available
      cwd: vscode.workspace.workspaceFolders?.[0]?.uri.fsPath,
    },
  };

  // Get trace level from configuration
  const config = vscode.workspace.getConfiguration("quartoRustLsp");
  const traceLevel = config.get<string>("trace.server", "off");

  // Configure the language client
  const clientOptions: LanguageClientOptions = {
    // Register for Quarto documents
    documentSelector: [{ scheme: "file", language: "quarto" }],

    // Sync configuration changes to the server
    synchronize: {
      configurationSection: "quartoRustLsp",
      // Notify server of file changes in the workspace
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.qmd"),
    },

    // Use our output channel for logs
    outputChannel: outputChannel,
    traceOutputChannel: outputChannel,

    // Initialization options passed to the server
    initializationOptions: {
      logLevel: config.get<string>("logLevel", "warn"),
    },
  };

  // Create and start the language client
  client = new LanguageClient(
    "quartoRustLsp",
    "Quarto Rust LSP",
    serverOptions,
    clientOptions
  );

  // Start the client (also launches the server)
  outputChannel.appendLine("Starting language server...");
  try {
    await client.start();
    outputChannel.appendLine("Language server started successfully");
  } catch (error) {
    const message = `Failed to start Quarto LSP: ${error}`;
    vscode.window.showErrorMessage(message);
    outputChannel.appendLine(`Error: ${message}`);
    return;
  }

  // Register command to restart the language server
  const restartCommand = vscode.commands.registerCommand(
    "quartoRustLsp.restartServer",
    async () => {
      outputChannel.appendLine("Restarting language server...");
      if (client) {
        await client.restart();
        outputChannel.appendLine("Language server restarted");
      }
    }
  );
  context.subscriptions.push(restartCommand);

  // Register command to show the output channel
  const showOutputCommand = vscode.commands.registerCommand(
    "quartoRustLsp.showOutput",
    () => {
      outputChannel.show();
    }
  );
  context.subscriptions.push(showOutputCommand);

  outputChannel.appendLine("Quarto Rust LSP extension activated");
}

/**
 * Deactivate the extension.
 *
 * This is called when the extension is deactivated (e.g., VS Code is closing).
 */
export async function deactivate(): Promise<void> {
  if (client) {
    outputChannel?.appendLine("Stopping language server...");
    await client.stop();
    client = undefined;
  }
}
