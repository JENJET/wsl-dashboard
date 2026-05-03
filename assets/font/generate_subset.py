#!/usr/bin/env python3
"""
Extract used icons from .slint files and generate subset font
"""
import os
import re
from pathlib import Path
from fontTools.subset import Subsetter, Options
from fontTools.ttLib import TTFont

# Configuration
SRC_DIR = Path(__file__).parent.parent.parent / "src"
FONT_SOURCE = Path(__file__).parent / "segoeicons.ttf"
OUTPUT_FILE = Path(__file__).parent / "icons.ttf"

def get_all_slint_files(dir_path):
    """Recursively find all .slint files"""
    slint_files = []
    for root, dirs, files in os.walk(dir_path):
        for file in files:
            if file.endswith('.slint'):
                slint_files.append(Path(root) / file)
    return slint_files

def extract_unicodes(files):
    """Extract unicode characters from .slint files"""
    unicodes = set()
    # Regex for unicode escape sequences: \u{XXXX}
    regex = r'\\u\{([0-9a-fA-F]+)\}'
    
    for file in files:
        try:
            with open(file, 'r', encoding='utf-8') as f:
                content = f.read()
                matches = re.findall(regex, content)
                for match in matches:
                    # Convert hex to integer
                    code_point = int(match, 16)
                    unicodes.add(code_point)
        except Exception as e:
            print(f"Error reading {file}: {e}")
    
    return sorted(unicodes)

def main():
    print(f"Scanning for .slint files in: {SRC_DIR}")
    slint_files = get_all_slint_files(SRC_DIR)
    print(f"Found {len(slint_files)} files.")
    
    print("Extracting used icons...")
    unicodes = extract_unicodes(slint_files)
    print(f"Found {len(unicodes)} unique icons:")
    for code in unicodes:
        print(f"  U+{code:04X}")
    
    if not unicodes:
        print("No icons found! Aborting subsetting to avoid empty font.")
        return
    
    print(f"\nSubsetting font from {FONT_SOURCE} to {OUTPUT_FILE}...")
    
    if not FONT_SOURCE.exists():
        print(f"Error: Source font not found at {FONT_SOURCE}")
        return
    
    try:
        # Load the font
        font = TTFont(FONT_SOURCE)
        
        # Create subsetter options
        options = Options()
        options.ignore_missing_glyphs = True
        
        # Create subsetter
        subsetter = Subsetter(options=options)
        
        # Prepare the list of code points
        code_points = list(unicodes)
        # Also add common ASCII characters that might be needed
        code_points.extend(range(32, 127))
        
        # Populate and subset
        subsetter.populate(unicodes=code_points)
        subsetter.subset(font)
        
        # Save the subsetted font
        font.save(OUTPUT_FILE)
        
        # Get file sizes
        original_size = FONT_SOURCE.stat().st_size
        new_size = OUTPUT_FILE.stat().st_size
        
        print(f"\nSuccessfully generated: {OUTPUT_FILE}")
        print(f"Original size: {original_size / 1024:.2f} KB")
        print(f"New size: {new_size / 1024:.2f} KB")
        print(f"Reduction: {(1 - new_size / original_size) * 100:.2f}%")
        
    except Exception as e:
        print(f"Error during font subsetting: {e}")
        import traceback
        traceback.print_exc()

if __name__ == "__main__":
    main()
