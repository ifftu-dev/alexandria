# alexandria/src/ — Vue frontend

Workspace rules and the general Vue/TS conventions are in the workspace `AGENTS.md`. This file holds the frontend gotchas.

- **Profiles first**: one device holds several network-bound profiles. The picker (`/profiles`, `pages/ProfileSelect.vue`) is the first screen whenever a profile exists.
- **Incompatible data**: when the backend refuses on-disk profile data (older index version or another network), `useProfiles().initialize()` returns `'incompatible'` and `App.vue` routes to `/incompatible-data` before anything else. Boot redirects wait for `router.isReady()` so the initial navigation cannot cancel them.
- **Profile state**: `composables/useProfiles.ts` is the canonical surface (unlock/lock/create/rename/delete/avatar) and exports the `onProfileReady` / `onProfileLocked` hooks. `useAuth.ts` is a back-compat shim over it; write new code against `useProfiles`.
- **Settings**: `useSetting<T>('ui.theme')` returns a two-way ref onto the backend registry. `scope='sync'` keys follow the user across devices, `scope='device'` keys stay local. See `docs/settings.md`.
- **Roles are a set**: a user is always a learner, plus optionally instructor and/or parent. Use `hasRole()` / `isParent()`; never compare a single role field.
- **UI primitives** come from the `@/components/ui` barrel; add new primitives there.
- **Sentinel ML runs in the backend** (`src-tauri/src/sentinel/`); `composables/useSentinel.ts` only buffers events and dispatches IPC. The LBP face embedder in `utils/sentinel/` is the one model left in TS.
