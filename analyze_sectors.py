#!/usr/bin/env python3
import sys

def read_sector(filename, sector_num, offset=0):
    """Lit un secteur spécifique depuis un fichier VHD"""
    sector_size = 512
    absolute_offset = offset + (sector_num * sector_size)
    
    with open(filename, 'rb') as f:
        f.seek(absolute_offset)
        return f.read(sector_size)

def format_hex(data, max_bytes=64):
    """Format des données en hexadécimal lisible"""
    result = []
    for i in range(0, min(len(data), max_bytes), 16):
        hex_part = ' '.join(f'{b:02X}' for b in data[i:i+16])
        ascii_part = ''.join(chr(b) if 32 <= b <= 126 else '.' for b in data[i:i+16])
        result.append(f"{i:04X}: {hex_part:<48} | {ascii_part}")
    return '\n'.join(result)

def analyze_fat_sector(data):
    """Analyse un secteur FAT exFAT"""
    print("=== Analyse secteur FAT ===")
    # Les premiers 8 bytes sont spéciaux en exFAT
    media_descriptor = int.from_bytes(data[0:4], 'little')
    second_entry = int.from_bytes(data[4:8], 'little')
    
    print(f"Media descriptor + padding: 0x{media_descriptor:08X}")
    print(f"Deuxième entrée FAT: 0x{second_entry:08X}")
    
    # Afficher les premières entrées FAT
    print("\nPremières entrées FAT:")
    for i in range(0, min(64, len(data)), 4):
        entry = int.from_bytes(data[i:i+4], 'little')
        cluster_num = i // 4
        if entry != 0:
            print(f"  Cluster {cluster_num}: 0x{entry:08X}")

def main():
    if len(sys.argv) != 4:
        print("Usage: python analyze_sectors.py <vhd_file> <partition_offset_hex> <sector_num>")
        sys.exit(1)
    
    vhd_file = sys.argv[1]
    partition_offset = int(sys.argv[2], 16)
    sector_num = int(sys.argv[3])
    
    print(f"Analyse du fichier: {vhd_file}")
    print(f"Offset de partition: 0x{partition_offset:X}")
    print(f"Secteur: {sector_num}")
    print()
    
    try:
        data = read_sector(vhd_file, sector_num, partition_offset)
        print(format_hex(data))
        print()
        
        # Si c'est un secteur FAT (secteur 128 et suivants pour Windows)
        if sector_num >= 128 and sector_num < 256:
            analyze_fat_sector(data)
            
    except Exception as e:
        print(f"Erreur: {e}")

if __name__ == "__main__":
    main()