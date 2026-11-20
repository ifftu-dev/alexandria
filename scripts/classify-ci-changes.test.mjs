import assert from "node:assert/strict"
import test from "node:test"

import { classifyChangedFiles } from "./classify-ci-changes.mjs"

test("checkout attributes run backend, security and release gates", () => {
  assert.deepEqual(classifyChangedFiles([".gitattributes"]), {
    backend: true,
    frontend: false,
    security: true,
    release: true,
  })
})

test("verifier-only changes run backend and security gates", () => {
  assert.deepEqual(classifyChangedFiles(["crates/alexandria-verify/src/lib.rs"]), {
    backend: true,
    frontend: false,
    security: true,
    release: false,
  })
})

test("media-only changes run backend and security gates", () => {
  assert.deepEqual(classifyChangedFiles(["crates/moq-media/src/lib.rs"]), {
    backend: true,
    frontend: false,
    security: true,
    release: false,
  })
})

test("command-only changes run backend gates", () => {
  assert.deepEqual(classifyChangedFiles(["src-tauri/src/commands/courses.rs"]), {
    backend: true,
    frontend: false,
    security: true,
    release: false,
  })
})

test("frontend-only changes do not request a Rust build", () => {
  assert.deepEqual(classifyChangedFiles(["src/pages/Home.vue"]), {
    backend: false,
    frontend: true,
    security: false,
    release: false,
  })
})

test("desktop UI fixtures and browser configuration run frontend gates", () => {
  for (const path of ["tests/desktop-ui/controls.spec.ts", "playwright.config.ts"]) {
    assert.deepEqual(classifyChangedFiles([path]), {
      backend: false,
      frontend: true,
      security: false,
      release: false,
    })
  }
})

test("the vendored SCORM runtime runs the browser checks as well as backend gates", () => {
  assert.deepEqual(classifyChangedFiles(["src-tauri/resources/scorm-again/scorm12.min.js"]), {
    backend: true,
    frontend: true,
    security: true,
    release: false,
  })
})

test("the package wrapper and bootstrap run the browser checks as well as backend gates", () => {
  assert.deepEqual(classifyChangedFiles(["src-tauri/src/package_serving/wrapper.js"]), {
    backend: true,
    frontend: true,
    security: true,
    release: false,
  })
})

test("the player's fixture package runs the backend test that pins its description", () => {
  assert.deepEqual(classifyChangedFiles(["tests/desktop-ui/package-player/package/quiz/index.html"]), {
    backend: true,
    frontend: true,
    security: true,
    release: false,
  })
})

test("future committee world and core packages run backend gates", () => {
  for (const path of [
    "committee/runtime/src/main.rs",
    "world/controller/src/main.rs",
    "core/protocol/src/lib.rs",
  ]) {
    const result = classifyChangedFiles([path])
    assert.equal(result.backend, true, path)
    assert.equal(result.security, true, path)
  }
})

test("release-sensitive inputs retain their release classification", () => {
  assert.equal(classifyChangedFiles(["scripts/check.sh"]).release, true)
  assert.equal(classifyChangedFiles(["src-tauri/tauri.android.conf.json"]).release, true)
  assert.equal(classifyChangedFiles(["src-tauri/src/commands/mod.rs"]).release, true)
  assert.equal(classifyChangedFiles(["src-tauri/build.rs"]).release, true)
  for (const path of [
    "src-tauri/src/tutoring/media/mod.rs",
    "src-tauri/src/tutoring/media/android.rs",
    "src-tauri/src/tutoring/media/ios.rs",
  ]) {
    assert.equal(classifyChangedFiles([path]).release, true, path)
  }
  assert.equal(classifyChangedFiles(["src-tauri/src/tutoring/manager.rs"]).release, false)
})
