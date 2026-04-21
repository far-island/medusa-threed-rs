# medusa-threed-rs

Rust gRPC engine for the **medusa-3d** module. Hosts the
`PointCloudService`, `MeshService`, and `ThreeDScanService` proto services
on a single TCP port (gRPC and gRPC-web both enabled).

Designed to be **spawned as a child process by the JavaFX `medusa-app`
launcher**, listening on `127.0.0.1` (loopback only). Not a long-running
daemon, not a multi-tenant service.

## Build

### Native (Linux)

```sh
cargo build --release
./target/release/medusa-threed-rs --help
```

### Cross-compile to Windows x86_64 (mingw, no Visual Studio)

Requires a `x86_64-w64-mingw32-gcc` toolchain (`apt install mingw-w64` on
Debian/Ubuntu) and the `x86_64-pc-windows-gnu` rustup target
(`rustup target add x86_64-pc-windows-gnu`).

```sh
cargo build --release --target x86_64-pc-windows-gnu
# Output: target/x86_64-pc-windows-gnu/release/medusa-threed-rs.exe
```

The resulting `.exe` links only to standard Windows system DLLs
(`kernel32`, `ntdll`, `ws2_32`, `msvcrt`, `bcryptprimitives`,
`api-ms-win-core-synch`, `userenv`). **No mingw runtime DLLs are
required** — the gcc/pthread runtime is statically linked. Ship the `.exe`
on its own.

## Launcher contract

This binary is intended to be spawned by a parent process (typically the
JavaFX `medusa-app`). The contract below is stable across releases — the
parent can rely on it.

### Command-line interface

```
medusa-threed-rs [--bind ADDR] [--port N]

OPTIONS
  --bind ADDR    Interface to bind. Default 127.0.0.1.
  --port N       TCP port to listen on. Default 50052.
                 Pass 0 to let the OS pick a free port; the actual port
                 is reported in the ready-line on stdout.
  --help, -h     Print help and exit 0.
  --version, -V  Print version and exit 0.
```

Unknown arguments cause exit code `2` with a usage message on stderr.

### Ready signal on stdout

After binding the listen socket and **immediately before** entering the
serve loop, the binary writes exactly this line on **stdout**, flushed:

```
READY port=<PORT>
```

This is the **first and only** stdout line written by the running server
(`--help` / `--version` use stdout but exit before serving). The parent
should:

1. Spawn the process with stdout and stderr piped.
2. Read stdout line-by-line until it sees a line matching the regex
   `^READY port=(\d+)$`.
3. Capture the matched port (useful when `--port 0` is used).
4. Connect the gRPC client to `<bind>:<port>`.

The parent must impose its own timeout on the read (recommended: 10 s
per orchestrator contract; 15 s for AIStation4070 cold-start with AV
scan). On timeout the parent should `destroyForcibly()` the process.

`stderr` carries the structured log: a `bind=<ADDR> port=<PORT>` line
right after `READY`, then shutdown breadcrumbs and any panic backtrace.
All `stderr` lines are prefixed with `[medusa-threed-rs]`.

### Graceful shutdown

The binary shuts down cleanly when **any** of the following occurs:

| Trigger              | Platform | Notes                                          |
|----------------------|----------|------------------------------------------------|
| `SIGTERM`            | Unix     | Sent by `kill <pid>`.                          |
| `SIGINT` / Ctrl-C    | Unix     | Same handling as SIGTERM.                      |
| `Ctrl-C`             | Windows  | Console only.                                  |
| `CTRL_BREAK_EVENT`   | Windows  | Sent via `GenerateConsoleCtrlEvent`.           |
| `CTRL_CLOSE_EVENT`   | Windows  | Console window closed / parent kills console.  |
| **stdin EOF**        | Both     | Parent closes the child's stdin handle.        |

**Recommended parent strategy (cross-platform, no JNA / native code):**
spawn the child with stdin piped, then on shutdown
`process.getOutputStream().close()` and wait up to 2 s for the child to
exit; on timeout `destroyForcibly()`.

On graceful shutdown the binary:

1. Stops accepting new connections.
2. Lets in-flight RPCs run to completion (no per-RPC deadline imposed).
3. Logs `shutdown complete` on stderr.
4. Exits with code `0`.

Fatal startup errors (bind failure, etc.) exit with code `1` and a single
`medusa-threed-rs: fatal: <reason>` line on stderr.

### Example: Java `ProcessBuilder` invocation

```java
ProcessBuilder pb = new ProcessBuilder(
        exePath.toString(),
        "--bind", "127.0.0.1",
        "--port", Integer.toString(port));
pb.redirectErrorStream(false);
Process p = pb.start();

// Read stdout until we see the ready signal (10 s timeout).
int actualPort;
try (BufferedReader out = new BufferedReader(new InputStreamReader(p.getInputStream()))) {
    String line;
    while ((line = out.readLine()) != null) {
        if (line.startsWith("READY port=")) {
            actualPort = Integer.parseInt(line.substring("READY port=".length()).trim());
            break;
        }
    }
}
// Drain stderr to a logger on a separate thread (structured log).

// ... use the gRPC client ...

// On shutdown:
p.getOutputStream().close();         // stdin EOF → graceful shutdown
if (!p.waitFor(2, TimeUnit.SECONDS)) {
    p.destroyForcibly();
}
```

## Services exposed

| Service               | Proto package           | Notes                              |
|-----------------------|-------------------------|------------------------------------|
| `PointCloudService`   | `farisland.threed.v1`   | Load PCD, export PTX, statistics.  |
| `MeshService`         | `farisland.threed.v1`   | Delaunay + grid meshing.           |
| `ThreeDScanService`   | `farisland.threed.v1`   | Scan dataset → point cloud stream. |

Both gRPC (HTTP/2) and gRPC-web (HTTP/1.1) are accepted on the same
port — the JavaFX WebView can connect via gRPC-web while a Java client
uses native gRPC, both at `127.0.0.1:<port>`.

## Tests

```sh
cargo test
# 42 tests: 35 service/algorithm + 7 launcher CLI parser cases.
```
