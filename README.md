# cleat-fgo-helper

A Rust CLI toolchain for Fate/Grand Order (FGO) resource management, APK modification, and translation workflow automation.

## Features

### Resource Pipeline
- Download and decrypt FGO game resources from JP/CN servers
- Automatic version detection and AssetStorage parsing
- Asset bundle extraction (Unity assets)

### APK Modification
- XAPK extraction and APK decompilation
- Inject native libraries and Smali patches
- Rebuild and sign APK

### Script System
- Convert between `.txt` script files and `.script` binary bundles
- Parse FGO story scripts (dialogue, choices, character names)

### Translation Workflow
- Export translatable text (dialogue, choices) to JSON
- Import translated JSON back into scripts
- Merge translation JSON with original exports
- Compare JP/CN scripts to find exclusive content
- Deduplicate translated scripts against official CN
- Deharmonize CN scripts (anti-censorship text replacements)

### Utilities
- Download Chaldea translation mapping data
- Generate character name mappings
- Script directory comparison and diff tools

## Installation

### Prerequisites
- [Rust](https://www.rust-lang.org/) (edition 2024)
- Java Runtime (for APK operations: apktool, apksigner)

### Build

```bash
git clone https://github.com/yourusername/cleat-fgo-helper.git
cd cleat-fgo-helper
cargo build --release
```

The binary will be at `target/release/fgo-helper`.

## Usage

```
fgo-helper <COMMAND>

Commands:
  apk       APK modification pipeline
  res       Download & decrypt FGO game resources
  scripts   Convert between .txt and .script bundles
  script    Export/import translatable text
  tools     Compare, dedup, and deharmonize scripts
  mappings  Download Chaldea translation mappings
  help      Print this message or the help of the given subcommand(s)
```

### Resource Download

```bash
# Download JP resources
fgo-helper res download jp

# Download CN resources (force re-download)
fgo-helper res download cn --force

# Check server version
fgo-helper res info jp
```

### Script Conversion

```bash
# Convert .txt files to binary bundle
fgo-helper scripts txt-to-bundle -i ./scripts -o bundle.script

# Extract binary bundle to .txt files
fgo-helper scripts bundle-to-txt -i bundle.script -o ./scripts
```

### Translation Export/Import

```bash
# Export dialogue and choices to JSON
fgo-helper script export -i data/jp/scripts -o data/export

# Import translated JSON back
fgo-helper script import --json-dir data/translated -o data/output

# Merge translations into exported JSON
fgo-helper script merge -t data/translated -o data/merged
```

### Script Tools

```bash
# Find JP-exclusive scripts not in CN
fgo-helper tools compare -j data/jp/scripts -c data/cn/scripts -o data/jp_only

# Remove CN-duplicate scripts from translated directory
fgo-helper tools dedup -c data/cn/scripts -t data/translated

# Deharmonize CN scripts
fgo-helper tools deharmonize -i data/cn/scripts -o data/cn_deharmonized
```

### APK Operations

```bash
# Setup: extract XAPK + decompile + inject patches
fgo-helper apk setup --xapk game.xapk

# Build: rebuild and sign APK
fgo-helper apk build

# Clean build artifacts
fgo-helper apk clean
```

### Mappings

```bash
# Download Chaldea translation mappings
fgo-helper mappings download -o data/mappings

# Generate character name mappings
fgo-helper tools scan-names -m data/mappings -o names.json
```

## Project Structure

```
src/
├── apk/          # APK modification (setup, build, clean)
├── bundle/       # Script bundle read/write
├── config/       # Configuration
├── crypto/       # AES/Rijndael decryption, hashing, key management
├── error/        # Error types
├── mappings/     # Chaldea translation data download
├── res/          # Resource pipeline (download, parse, version)
├── scripts/      # Script parsing, export/import, merge, tools
├── unity/        # Unity asset extraction
└── util/         # Utility functions
```

## Dependencies

Key dependencies:
- `clap` — CLI argument parsing
- `tokio` / `reqwest` — Async HTTP for resource downloads
- `simple-rijndael` — AES decryption for FGO assets
- `flate2` / `bzip2` — Decompression
- `unity-asset` — Unity asset parsing
- `peg` — PEG parser generator for script syntax
- `rayon` — Parallel processing

## License

MIT
