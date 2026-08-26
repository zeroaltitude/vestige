#!/usr/bin/env node

const https = require('https');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { execSync } = require('child_process');

/**
 * True when this script is running inside the vestige source monorepo rather than as an
 * installed dependency. In-tree the binaries come from `cargo build --release -p vestige-mcp`,
 * so fetching a prebuilt release archive is pointless — and failing here breaks every
 * workspace `pnpm install`, including `apps/dashboard`, whose only runtime dependency is
 * `three` and which never touches the MCP server.
 *
 * Deliberately not a bare `process.env.CI` check: that would also silently skip the download
 * for real consumers who install the published package on their own CI, swapping a clear
 * install-time error for a confusing "binary not found" at first run.
 */
function isSourceMonorepo() {
  // scripts/ -> vestige-mcp-npm/ -> packages/ -> repo root
  const repoRoot = path.join(__dirname, '..', '..', '..');
  try {
    const rootManifest = path.join(repoRoot, 'package.json');
    if (!fs.existsSync(rootManifest)) return false;
    if (JSON.parse(fs.readFileSync(rootManifest, 'utf8')).name !== 'vestige') return false;
    return fs.existsSync(path.join(repoRoot, 'crates', 'vestige-mcp', 'Cargo.toml'));
  } catch {
    return false;
  }
}

if (process.env.VESTIGE_SKIP_BINARY_DOWNLOAD === '1') {
  console.log('VESTIGE_SKIP_BINARY_DOWNLOAD=1 — skipping prebuilt binary download.');
  process.exit(0);
}

if (isSourceMonorepo()) {
  console.log('Detected the vestige source monorepo — skipping prebuilt binary download.');
  console.log('Build the binaries from source instead: cargo build --release -p vestige-mcp');
  process.exit(0);
}

const VERSION = require('../package.json').version;

// GitHub release tag hosting the prebuilt binaries.
//
// INVARIANT: this tag must publish an archive for *every* target `target` below can name.
// Do not derive it from VERSION — the npm package version and the binary release tag are not
// kept in lockstep upstream, and v2.0.0 (the current VERSION) published only the Windows zip.
// Do not bump it to a release that ships a subset of targets either; check first with
// `gh release view v<tag> --repo samvallad33/vestige --json assets -q '.assets[].name'`.
const BINARY_VERSION = process.env.VESTIGE_BINARY_VERSION || '2.3.0';
const PLATFORM = os.platform();
const ARCH = os.arch();

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
 * Remove the file we opened for a download that never produced bytes.
 *
 * Synchronous on purpose: main() calls process.exit(1) from its catch handler, so a
 * callback-based fs.unlink is never given a chance to run and the empty archive survives.
 */
function discardPartialFile(file, dest) {
  file.destroy();
  try {
    fs.unlinkSync(dest);
  } catch {
    // Already gone, or never created — nothing to clean up.
  }
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
          discardPartialFile(file, dest);
          reject(new Error(`Download failed: HTTP ${response.statusCode}`));
          return;
        }

        response.pipe(file);
        file.on('finish', () => {
          file.close();
          resolve();
        });
      }).on('error', (err) => {
        discardPartialFile(file, dest); // Delete partial file
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
    process.exit(1);
  }
}

main();
