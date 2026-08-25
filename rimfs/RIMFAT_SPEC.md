# RimFAT Filesystem Specification

RimFAT is a high-reliability extension of the standard FAT filesystem (FAT12/16/32) designed for the RIM ecosystem. It maintains bit-level compatibility with standard FAT tools while adding integrity verification, crash resistance, and performance hints.

## 1. Volume Identification

A RimFAT volume is identified by its **OEM Name** in the Boot Sector (VBR).

| Field        | Offset | Value        | Description                                 |
| :----------- | :----- | :----------- | :------------------------------------------ |
| `BS_OEMName` | 0x03   | `"RIMFAT  "` | Identifies the volume as RimFAT compatible. |

## 2. Integrity Mechanisms

RimFAT provides two layers of integrity verification: metadata-level and filesystem-level.

### 2.1 Metadata Integrity (Directory Entries)

Each directory entry (including LFN segments) is protected by a checksum to detect bit-rot or accidental corruption.

- **Algorithm**: 16-bit CRC reduced via `modulo 100`.
- **Storage**: The resulting value (0-99) is stored in the `creation_time_tenth` field (offset 0x0D).
- **Standard Compatibility**:
  - The `nt_reserved` byte (offset 0x0C) is **always set to 0**.
  - By keeping the tenths-of-second field within the 0-199 range, RimFAT avoids many "invalid field" errors in strict tools like `fsck.fat`.
- **Coverage**: The checksum covers the entire 32-byte SFN entry (with checksum fields masked) and all associated 32-byte LFN entries.

### 2.2 Global FAT Integrity

To detect corruption of the allocation table itself, RimFAT stores a global checksum.

- **Algorithm**: CRC32 of the entire active FAT table (excluding the mirrored copies).
- **Storage**: `fat_checksum` field in the `FSINFO` sector (offset 476, length 4 bytes).
- **Verification**: Verified at mount time. If the calculated CRC32 does not match the stored value, the volume is considered corrupted.

## 3. Crash Resistance (Transactions)

RimFAT implements a basic transaction mechanism using a dedicated sector to handle interrupted writes.

- **Transaction Sector**: Always located at **Sector 12** (within the reserved sectors area).
- **Dirty Flag**:
  - Before starting a complex write operation (e.g., in `FatInjector`), the string `"DIRTY"` is written to the first 5 bytes of Sector 12.
  - Upon successful completion and global checksum update, Sector 12 is cleared (zero-filled).
- **Recovery**: If a volume is mounted and the transaction sector is found to be `"DIRTY"`, the system knows that the last operation was interrupted.

## 4. Performance Optimizations

### 4.1 Contiguous Attribute (Hint)

RimFAT introduces a high-performance read hint for files known to be non-fragmented.

- **Flag**: Value **+100** in the `creation_time_tenth` field (offset 0x0D).
- **Encoding**: `creation_time_tenth = (CRC16 % 100) + (is_contiguous ? 100 : 0)`.
- **Semantic**: The file is likely stored as a single contiguous cluster chain.
- **Verification**: The hint is only trusted if the metadata checksum (CRC % 100) is valid.
- **Safety**: Readers SHOULD verify the first few clusters in the FAT table before taking the fast-path.
- **Benefit**: Readers can skip full FAT chain lookups and perform a single large IO operation.
- **Standard Compatibility**:
  - The value remains within 0-199, which is standard for `creation_time_tenth`.
  - No non-standard bits are used in the `Attributes` field, ensuring compatibility with Windows/chkdsk.

---

_Status: Draft V1.0_
