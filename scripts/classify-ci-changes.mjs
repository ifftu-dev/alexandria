const BACKEND_PREFIXES = [
  ".github/workflows/",
  "cli/",
  "committee/",
  "core/",
  "crates/",
  "patches/",
  "scripts/",
  "src-tauri/",
  "world/",
]

const BACKEND_FILES = new Set([".gitattributes", "Cargo.lock", "Cargo.toml"])
const FRONTEND_PREFIXES = ["src/", "tests/desktop-ui/"]
const FRONTEND_FILES = new Set([
  "index.html",
  "package-lock.json",
  "package.json",
  "vite.config.ts",
  "playwright.config.ts",
])

function isTypeScriptConfig(path) {
  return /^tsconfig.*\.json$/.test(path)
}

function isReleaseInput(path) {
  return (
    /^\.github\/workflows\/(?:release-|validate-)/.test(path) ||
    [
      ".gitattributes",
      ".github/workflows/ci.yml",
      ".github/workflows/desktop-shared.yml",
      ".github/workflows/mobile-shared.yml",
      "package-lock.json",
      "package.json",
      "src-tauri/Cargo.toml",
      "src-tauri/build.rs",
      "src-tauri/src/commands/mod.rs",
    ].includes(path) ||
    path.startsWith("scripts/") ||
    // Which media adapter each platform's calls run on, and what it can do.
    path.startsWith("src-tauri/src/tutoring/media/") ||
    /^src-tauri\/tauri.*\.json$/.test(path)
  )
}

export function classifyChangedFiles(paths) {
  const backend = paths.some(
    (path) =>
      BACKEND_FILES.has(path) || BACKEND_PREFIXES.some((prefix) => path.startsWith(prefix)),
  )
  const frontend = paths.some(
    (path) =>
      FRONTEND_FILES.has(path) ||
      FRONTEND_PREFIXES.some((prefix) => path.startsWith(prefix)) ||
      isTypeScriptConfig(path),
  )
  return {
    backend,
    frontend,
    security: backend,
    release: paths.some(isReleaseInput),
  }
}

if (process.argv[1] && import.meta.url === new URL(process.argv[1], "file:").href) {
  const classification = classifyChangedFiles(process.argv.slice(2))
  for (const [surface, matched] of Object.entries(classification)) {
    process.stdout.write(`${surface}=${matched}\n`)
  }
}
