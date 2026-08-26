#!/usr/bin/env node

const https = require('https');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { execSync } = require('child_process');

const VERSION = require('../package.json').version;
// GitHub release tag the prebuilt binaries are fetched from. Derived from the package
// version so the two can never silently drift (this file previously announced v2.0.0 while
// requesting the v1.1.3 assets, a tag that does not exist -> unconditional 404).
// Override with VESTIGE_BINARY_VERSION when a package version has no matching release tag.
const BINARY_VERSION = process.env.VESTIGE_BINARY_VERSION || VERSION;
const PLATFORM = os.platform();
const ARCH = os.arch();

/**
 * True when this script is running from the Vestige source monorepo rather than from an
 * installed copy of the published npm package. All four conditions have to hold, and a
 * published tarball (which unpacks to node_modules/vestige-mcp-server) satisfies none of
 * them, so this can never accidentally no-op a real consumer install.
 */
function isWorkspaceCheckout() {
  const packageRoot = path.resolve(__dirname, '..'); // packages/vestige-mcp-npm
  const repoRoot = path.resolve(packageRoot, '..', '..'); // repo root
  return (
    path.basename(packageRoot) === 'vestige-mcp-npm' &&
    path.basename(path.dirname(packageRoot)) === 'packages' &&
    fs.existsSync(path.join(repoRoot, 'pnpm-workspace.yaml')) &&
    fs.existsSync(path.join(repoRoot, 'crates', 'vestige-mcp', 'Cargo.toml'))
  );
}

// Nothing in the monorepo consumes the prebuilt binary — inside the source tree it is built
// with `cargo build --release -p vestige-mcp`. Downloading a release asset there is pointless,
// and it is what breaks the Dashboard Build CI job: pnpm resolves to the workspace root even
// when invoked from apps/dashboard, so the dashboard install runs this postinstall.
if (
  process.env.VESTIGE_SKIP_BINARY_DOWNLOAD === '1' ||
  process.env.VESTIGE_MCP_SKIP_DOWNLOAD === '1' ||
  isWorkspaceCheckout()
) {
  console.log('Skipping Vestige MCP binary download (source checkout or explicit skip).');
  console.log('Build it from source instead: cargo build --release -p vestige-mcp');
  process.exit(0);
}

const PLATFORM_MAP = {
  darwin: 'apple-darwin',
  linux: 'unknown-linux-gnu',
  win32: 'pc-windows-msvc',
};

const ARCH_MAP = {
  x64: 'x86_64',
  arm64: 'aarch64',
};

const platformStr = PLATFORM_MAP[PLATFORM];
const archStr = ARCH_MAP[ARCH];

if (!platformStr || !archStr) {
  console.error(`Unsupported platform: ${PLATFORM}-${ARCH}`);
  console.error('Supported: darwin/linux/win32 on x64/arm64');
  process.exit(1);
}

const target = `${archStr}-${platformStr}`;
const isWindows = PLATFORM === 'win32';
const archiveExt = isWindows ? 'zip' : 'tar.gz';
const archiveName = `vestige-mcp-${target}.${archiveExt}`;
const downloadUrl = `https://github.com/samvallad33/vestige/releases/download/v${BINARY_VERSION}/${archiveName}`;

const targetDir = path.join(__dirname, '..', 'bin');
const archivePath = path.join(targetDir, archiveName);

console.log(`Installing Vestige MCP v${VERSION} for ${target}...`);

// Ensure bin directory exists
if (!fs.existsSync(targetDir)) {
  fs.mkdirSync(targetDir, { recursive: true });
}

/**
 * Download a file following redirects (GitHub releases use redirects)
 */
function download(url, dest) {
  return new Promise((resolve, reject) => {
    const file = fs.createWriteStream(dest);

    const request = (currentUrl) => {
      https.get(currentUrl, (response) => {
        // Handle redirects (GitHub uses 302)
        if (response.statusCode === 301 || response.statusCode === 302) {
          const redirectUrl = response.headers.location;
          if (!redirectUrl) {
            reject(new Error('Redirect without location header'));
            return;
          }
          request(redirectUrl);
          return;
        }

        if (response.statusCode !== 200) {
          reject(new Error(`Download failed: HTTP ${response.statusCode}`));
          return;
        }

        response.pipe(file);
        file.on('finish', () => {
          file.close();
          resolve();
        });
      }).on('error', (err) => {
        fs.unlink(dest, () => {}); // Delete partial file
        reject(err);
      });
    };

    request(url);
  });
}

/**
 * Extract archive based on platform
 */
function extract(archivePath, destDir) {
  if (isWindows) {
    // Use PowerShell to extract zip on Windows
    execSync(
      `powershell -Command "Expand-Archive -Path '${archivePath}' -DestinationPath '${destDir}' -Force"`,
      { stdio: 'inherit' }
    );
  } else {
    // Use tar on Unix
    execSync(`tar -xzf "${archivePath}" -C "${destDir}"`, { stdio: 'inherit' });
  }
}

/**
 * Make binaries executable (Unix only)
 */
function makeExecutable(binDir) {
  if (isWindows) return;

  const binaries = ['vestige-mcp', 'vestige', 'vestige-restore'];
  for (const bin of binaries) {
    const binPath = path.join(binDir, bin);
    if (fs.existsSync(binPath)) {
      fs.chmodSync(binPath, 0o755);
    }
  }
}

async function main() {
  try {
    // Download
    console.log(`Downloading from ${downloadUrl}...`);
    await download(downloadUrl, archivePath);
    console.log('Download complete.');

    // Extract
    console.log('Extracting binaries...');
    extract(archivePath, targetDir);

    // Cleanup archive
    fs.unlinkSync(archivePath);

    // Make executable
    makeExecutable(targetDir);

    // Verify installation
    const mcpBinary = path.join(targetDir, isWindows ? 'vestige-mcp.exe' : 'vestige-mcp');
    const cliBinary = path.join(targetDir, isWindows ? 'vestige.exe' : 'vestige');

    if (!fs.existsSync(mcpBinary)) {
      throw new Error('vestige-mcp binary not found after extraction');
    }

    console.log('');
    console.log('Vestige MCP installed successfully!');
    console.log('');
    console.log('Binaries installed:');
    console.log(`  - vestige-mcp: ${mcpBinary}`);
    if (fs.existsSync(cliBinary)) {
      console.log(`  - vestige:     ${cliBinary}`);
    }
    console.log('');
    console.log('Next steps:');
    console.log('  1. Add to Claude: claude mcp add vestige vestige-mcp -s user');
    console.log('  2. Restart Claude');
    console.log('  3. Test with: "remember that my favorite color is blue"');
    console.log('');

  } catch (err) {
    console.error('');
    console.error('Installation failed:', err.message);
    console.error('');
    console.error('Manual installation:');
    console.error(`  1. Download: ${downloadUrl}`);
    console.error(`  2. Extract to: ${targetDir}`);
    console.error('  3. Ensure binaries are executable (chmod +x on Unix)');
    console.error('');
    console.error('Other options:');
    console.error('  - Pick a different release tag: VESTIGE_BINARY_VERSION=<x.y.z> npm install');
    console.error('  - Skip the download entirely:   VESTIGE_MCP_SKIP_DOWNLOAD=1 npm install');
    console.error('    (the vestige-mcp / vestige wrappers will then error until a binary exists)');
    console.error('');
    process.exit(1);
  }
}

main();
