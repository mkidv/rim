#!/usr/bin/env python3
import sys

def read_cluster(filename, cluster_num, partition_offset, cluster_size=32768, cluster_heap_offset_sectors=256, sector_size=512):
    """Lit un cluster depuis un fichier VHD"""
    # Calcul de l'offset du cluster
    # cluster_heap_offset = offset du début du heap de clusters
    # cluster_offset = cluster_heap_offset + (cluster_num - 2) * cluster_size
    cluster_heap_offset_bytes = cluster_heap_offset_sectors * sector_size
    cluster_offset = cluster_heap_offset_bytes + (cluster_num - 2) * cluster_size
    absolute_offset = partition_offset + cluster_offset
    
    with open(filename, 'rb') as f:
        f.seek(absolute_offset)
        return f.read(cluster_size)

def analyze_directory_entries(data):
    """Analyse les entrées de répertoire exFAT"""
    print("=== Analyse des entrées de répertoire ===")
    
    for i in range(0, len(data), 32):
        entry = data[i:i+32]
        if len(entry) < 32:
            break
            
        entry_type = entry[0]
        
        if entry_type == 0x00:
            print(f"Entrée {i//32}: End of Directory")
            break
        elif entry_type == 0x81:
            # Bitmap entry
            first_cluster = int.from_bytes(entry[20:24], 'little')
            size = int.from_bytes(entry[24:32], 'little')
            print(f"Entrée {i//32}: Bitmap Entry")
            print(f"  First cluster: {first_cluster}")
            print(f"  Size: {size} bits")
        elif entry_type == 0x82:
            # Upcase table entry
            first_cluster = int.from_bytes(entry[20:24], 'little')
            checksum = int.from_bytes(entry[4:8], 'little')
            print(f"Entrée {i//32}: Upcase Table Entry")
            print(f"  Checksum: 0x{checksum:08X}")
            print(f"  First cluster: {first_cluster}")
        elif entry_type == 0x83:
            # Volume label entry
            label_len = entry[1]
            label_bytes = entry[2:2+label_len*2]  # UTF-16
            try:
                label = label_bytes.decode('utf-16le')
            except:
                label = "<invalid>"
            print(f"Entrée {i//32}: Volume Label Entry")
            print(f"  Label: '{label}'")
        elif entry_type == 0xA0:
            # GUID entry
            guid_bytes = entry[20:36]
            print(f"Entrée {i//32}: GUID Entry")
            print(f"  GUID: {guid_bytes.hex()}")
        elif entry_type == 0x85:
            # File entry
            print(f"Entrée {i//32}: File Entry")
        else:
            print(f"Entrée {i//32}: Type inconnu 0x{entry_type:02X}")
        
        print()

def format_hex(data, max_bytes=256):
    """Format des données en hexadécimal lisible"""
    result = []
    for i in range(0, min(len(data), max_bytes), 16):
        hex_part = ' '.join(f'{b:02X}' for b in data[i:i+16])
        ascii_part = ''.join(chr(b) if 32 <= b <= 126 else '.' for b in data[i:i+16])
        result.append(f"{i:04X}: {hex_part:<48} | {ascii_part}")
    return '\n'.join(result)

def main():
    if len(sys.argv) != 4:
        print("Usage: python analyze_root_cluster.py <vhd_file> <partition_offset_hex> <cluster_num>")
        sys.exit(1)
    
    vhd_file = sys.argv[1]
    partition_offset = int(sys.argv[2], 16)
    cluster_num = int(sys.argv[3])
    
    print(f"Analyse du cluster {cluster_num} dans: {vhd_file}")
    print(f"Offset de partition: 0x{partition_offset:X}")
    print()
    
    try:
        data = read_cluster(vhd_file, cluster_num, partition_offset)
        print("=== Données brutes (premiers 256 bytes) ===")
        print(format_hex(data))
        print()
        
        analyze_directory_entries(data)
            
    except Exception as e:
        print(f"Erreur: {e}")

if __name__ == "__main__":
    main()