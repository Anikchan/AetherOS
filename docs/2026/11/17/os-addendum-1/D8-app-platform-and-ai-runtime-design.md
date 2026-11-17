# D8 — App Platform and AI Runtime Design

Contracts first (section 2), then flows and rules. Feature requirements: F8, F9, F10. Process rules: D3. Measurement: D6.

## 1. Components

| Component | Responsibility | Process |
| --- | --- | --- |
| `nexa-pkg` | Install, update, remove; manifest checks; file registry | System service (installs need elevation through a permission prompt) |
| `nexa-sandbox` | Builds sandbox profiles from permissions; applies them before an app starts | Library used by the runner |
| `nexa-runner` | Starts apps by kind (native, AppImage, cross-platform); uses zygote (F11 LA-02) | Helper process per user session |
| `nexa-compat` | Manages the Windows engine versions and prefixes | User service |
| `nexa-permissions` | Broker for file pickers, camera, microphone, location, and notifications | User service (D-Bus interface) |
| `nexa-ai` | AI runtime lifecycle, model loading, generation requests | User service, managed by `nexa-lifecycled` as an app |
| `nexa-apps-store` | Software app UI | Shell app |

## 2. Contracts (Rust)

```rust
pub struct AppId(pub String);

pub enum PackageKind { NativeBundle, AppImage, WindowsInstaller, CrossPlatformBundle, MacosPackage }

pub enum Permission {
    Network,
    Files { scope: FileScope },
    Camera,
    Microphone,
    Location,
    Notifications,
    Background,
}

pub enum FileScope { PickedByUser, AppFolder, Home }

pub struct PackageManifest {
    pub id: AppId,
    pub name: String,
    pub version: String,
    pub kind: PackageKind,
    pub permissions: Vec<Permission>,
    pub entry: String,
    pub installed_bytes: u64,
    pub signature: Option<Signature>,      // native bundles are signed
}

pub enum InstallOutcome {
    Installed(AppId),
    NeedsPermission(Vec<Permission>),
    Unsupported { reason: String, alternatives: Vec<String> },
}

pub trait PackageInstaller {
    fn inspect(&self, file: &std::path::Path) -> Result<PackageManifest, InstallError>;
    fn install(&self, manifest: &PackageManifest, file: &std::path::Path) -> Result<InstallOutcome, InstallError>;
    fn uninstall(&self, id: &AppId) -> Result<(), InstallError>;
}

pub enum EngineId { NativeRunner, WineEngine { version: String }, CrossPlatformRunner }

pub trait EngineRegistry {
    fn engine_for(&self, kind: PackageKind) -> Option<EngineId>;
}

pub struct SandboxProfile {
    pub filesystem_read: Vec<std::path::PathBuf>,
    pub filesystem_write: Vec<std::path::PathBuf>,
    pub network: bool,
    pub devices: Vec<String>,              // camera, microphone, etc.
}

pub trait Sandbox {
    fn profile_for(&self, manifest: &PackageManifest) -> SandboxProfile;
}

pub trait Runner {
    fn launch(&self, id: &AppId, profile: &SandboxProfile) -> Result<ProcessHandle, LaunchError>;
}

pub enum AiState { NotInstalled, Unloaded, Loading, Ready, Generating, Stopping }

pub struct GenerateRequest { pub prompt: String, pub max_tokens: u32, pub time_limit: std::time::Duration }

pub trait AiRuntime {
    fn state(&self) -> AiState;
    fn load(&mut self) -> Result<(), AiError>;
    fn unload(&mut self) -> Result<(), AiError>;
    fn generate(&mut self, request: GenerateRequest) -> Result<Vec<String>, AiError>; // tokens, streamed in real use
    fn stop(&mut self);
}

#[derive(Debug)]
pub enum InstallError { Io(std::io::Error), BadSignature, ChecksumMismatch, DiskFull, Conflict(String) }
#[derive(Debug)]
pub enum LaunchError { EngineMissing(String), SandboxFailed(String), NotInstalled }
#[derive(Debug)]
pub enum AiError { NotInstalled, HardwareUnsupported(String), OutOfMemory, Timeout, Io(std::io::Error) }
```

Notes:
- `inspect` never changes the system. It only reads the file.
- `install` is atomic: files are staged, checked, then moved into place. A failure removes the staging folder.
- `generate` returns tokens as a stream in the real implementation. The contract shows a simple form for planning.

## 3. Storage Layout

| Path | Contents |
| --- | --- |
| `/Applications` | System-wide native bundles (installed by administrator) |
| `~/Applications` | User native bundles |
| `~/.local/share/nexa/apps/<app-id>/data` | App data folder (the only folder an app can write by default) |
| `~/.local/share/nexa/prefixes/<app-id>` | Windows program prefix (private registry and files) |
| `/usr/lib/nexa/engines/wine-<version>` | Windows engine (system, versioned) |
| `/usr/lib/nexa/engines/runner-<version>` | Native runner (system, versioned) |
| `~/.local/share/nexa/ai/models/<model-id>` | AI model files (user; removed with the AI feature) |
| `~/.local/share/nexa/install-registry.db` | File registry for every installed app (used by uninstall) |

Paths are created by `nexa-pkg` on first use. Removing an app deletes its registry entries and folders.

## 4. Install Flow

1. `inspect` reads the file and returns a manifest. Unknown or unsupported kinds return `Unsupported` with the message from F9 section 5.
2. For native bundles, the signature is checked. Failure stops the install.
3. The permission prompt lists each permission (`NeedsPermission`). The user accepts or cancels.
4. Files are staged in `~/.local/share/nexa/staging/<app-id>`.
5. Checksums are verified against the manifest.
6. Files move into the target folder in one step.
7. Registry entries are written (every file path and hash).
8. The launcher refreshes. The app appears.
9. Failure at any step removes staging and any partial registry entries.

## 5. Launch Flow and Sandbox Rules

1. The launcher sends a launch request to `nexa-lifecycled` (D3 protocol).
2. `EngineRegistry` selects the engine by package kind.
3. `Sandbox::profile_for` builds the profile from the manifest permissions.
4. `Runner::launch` starts the app inside the sandbox. Native apps use Landlock for file access, user and mount namespaces, and a seccomp filter. Windows programs run in their prefix under the Windows engine with the same restrictions where the engine allows.
5. The app's window appears; the lifecycle state becomes Foreground (D3).

Sandbox rules:
- Files outside the app's folder need a file picker through `nexa-permissions`. The picker returns a handle; the app never sees the rest of the folder tree.
- Network is off unless the manifest asks for it and the user accepts.
- Devices (camera, microphone) are granted only when the user accepts the matching permission.
- Windows programs are isolated by prefix. Their access to the rest of the home folder is limited by the same file rules where the engine supports them; this is a known limit (F3 section 4).

## 6. AI Runtime State Machine

```
NotInstalled --(install)--> Unloaded --(first request)--> Loading --> Ready
Ready --(request)--> Generating --(done or stop)--> Ready
Ready --(2 min idle)--> Unloaded
Any --(remove)--> NotInstalled
Loading --(out of memory or hardware gate)--> Unloaded (with message)
```

Rules:
- `Unloaded` uses 0 MB of model memory. The runtime process may also be suspended by F2.
- A request in `Generating` is stopped within 1 second when the user presses Stop (AI-09).
- On battery, a request longer than the set limit asks the user first (AI-07, F7 policy).
- `generate` never starts when `NotInstalled`, and never sends network traffic (AI-06 in F10, G-AI3 in D6).

## 7. Failure Modes

| Failure | Response |
| --- | --- |
| Install interrupted (power loss) | Staging is removed on next start; the registry is not updated until the final step |
| Signature wrong | Install stops; the file is not copied |
| Windows engine missing | App shows "Engine is being installed" and offers to install it |
| Sandbox cannot be applied (kernel feature missing) | Launch stops with an explanation; the app does not start unsandboxed |
| AI out of memory | Unload, show message, suggest a smaller model |
| Model file corrupt | Checksum fails; model removed; user can reinstall |

## 8. Tests

| Test | Type | Pass criterion |
| --- | --- | --- |
| Install, launch, uninstall native bundle | Integration | No files left after uninstall (registry diff is empty) |
| Interrupted install | Integration (kill during install) | No partial app; staging removed |
| Sandbox file access | Integration | Access outside allowed folders fails |
| Permission prompt | Integration | Denied permission stays denied after restart |
| Windows prefix removal | Integration (v1) | Only the target prefix removed |
| macOS package detection | Unit | Message and no install action |
| AI state machine | Unit | Every transition in section 6 |
| AI unload after idle | Integration | Memory returns to 0 MB model use within 2 minutes |
| AI no network | Integration | Monitor shows no outgoing connection during generation |
