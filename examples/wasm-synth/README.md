# RIM WebAssembly Client-Side Disk Synthesizer Demo

Proof-of-concept demonstrating the portable RIM engine running entirely client-side inside a web browser to synthesize a partitioned disk image (GPT + FAT32 + EXT4) in memory.

---

## Architecture

```
Browser UI (HTML5 / Vanilla JS)
  ¦
  ? WebAssembly (wasm32-unknown-unknown)
wasm-synth (thin adapter / C ABI)
  ¦
  ? Portable RIM Engine (no_std + alloc)
rimgen::build_on_io()
  +-- rimpart (GPT partitioning & table validation)
  +-- rimfs-fat (FAT32 formatting & /README.TXT injection)
  +-- rimfs-ext (EXT4 formatting & /hello.txt, /etc/rim.conf, /hello_link injection)
  +-- rimio (MemRimIO 64 MiB in-memory buffer)
```

---

## Build Instructions

### 1. Prerequisites
Ensure the `wasm32-unknown-unknown` target is installed:
```bash
rustup target add wasm32-unknown-unknown
```

### 2. Build WebAssembly Module
From the workspace root:
```bash
# Build the optimized release WebAssembly artifact
cargo build -p wasm-synth --target wasm32-unknown-unknown --release

# Copy the wasm binary to the demo directory
cp target/wasm32-unknown-unknown/release/wasm_synth.wasm examples/wasm-synth/
```
*(On Windows PowerShell: `Copy-Item target\wasm32-unknown-unknown\release\wasm_synth.wasm examples\wasm-synth\ -Force`)*

---

## Running the Browser Demo

Because WebAssembly files must be loaded via `fetch()`, serve the directory with any local HTTP server:

```bash
cd examples/wasm-synth
python -m http.server 8080
```

Then open [http://localhost:8080](http://localhost:8080) in your browser:
1. Click **"Synthesize 64 MiB Demo Disk"**.
2. Observe the synthesis execution and GPT validation in the live log.
3. Click **"Download .img"** to save the generated `rim-wasm-demo.img`.

---

## Generated Image Layout

| Partition | Type / Filesystem | Size | Volume UUID | Injected Payloads |
| :--- | :--- | :--- | :--- | :--- |
| **Partition 1** | ESP / `FAT32` | 32 MiB | `ABCD-1234` | `/README.TXT` |
| **Partition 2** | Linux / `EXT4` | 24 MiB | `12345678-1234-5678-1234-567812345678` | `/hello.txt`, `/etc/rim.conf`, `/hello_link` (symlink) |

---

## Validation with Host Tooling

The downloaded `rim-wasm-demo.img` can be inspected natively with RIM CLI or Linux loopback:

```bash
# Inspect partition table with RIM
rim inspect rim-wasm-demo.img

# Validate filesystems with RIM checker
rim check rim-wasm-demo.img --verbose

# Inspect on Linux
sudo losetup -P -f rim-wasm-demo.img
sudo fsck.vfat -v -n /dev/loop0p1
sudo e2fsck -n -f /dev/loop0p2
sudo losetup -d /dev/loop0
```
