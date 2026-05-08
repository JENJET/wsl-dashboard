#!/usr/bin/env python
"""
Extract used icons from .slint files and generate subset font
Compatible with Python 2.7 and Python 3.x
"""
from __future__ import print_function
import os
import re
import sys

# Try to import pathlib (Python 3.4+) or use fallback for Python 2
try:
    from pathlib import Path
except ImportError:
    # Fallback for Python 2
    class Path:
        def __init__(self, *parts):
            self._path = os.path.join(*parts)
        
        def __truediv__(self, other):
            return Path(self._path, other)
        
        def __div__(self, other):
            return Path(self._path, other)
        
        def __str__(self):
            return self._path
        
        def exists(self):
            return os.path.exists(self._path)
        
        def stat(self):
            return os.stat(self._path)
        
        def parent(self):
            return Path(os.path.dirname(self._path))

try:
    from fontTools.subset import Subsetter, Options
    from fontTools.ttLib import TTFont
except ImportError:
    print("Error: fonttools is required. Install with: pip install fonttools", file=sys.stderr)
    sys.exit(1)

# Configuration
SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
SRC_DIR = Path(SCRIPT_DIR, '..', '..', 'src')
FONT_SOURCE = Path(SCRIPT_DIR, 'segoeicons.ttf')
OUTPUT_FILE = Path(SCRIPT_DIR, 'icons.ttf')

def get_all_slint_files(dir_path):
    """Recursively find all .slint files"""
    slint_files = []
    dir_path_str = str(dir_path) if hasattr(dir_path, '_path') else dir_path
    for root, dirs, files in os.walk(dir_path_str):
        for file in files:
            if file.endswith('.slint'):
                slint_files.append(Path(root, file))
    return slint_files

def extract_unicodes(files):
    """Extract unicode characters from .slint files"""
    unicodes = set()
    # Regex for unicode escape sequences: \u{XXXX}
    regex = r'\\u\{([0-9a-fA-F]+)\}'
    
    for file in files:
        try:
            file_path = str(file) if hasattr(file, '_path') else file
            with open(file_path, 'r', encoding='utf-8') as f:
                content = f.read()
                matches = re.findall(regex, content)
                for match in matches:
                    # Convert hex to integer
                    code_point = int(match, 16)
                    unicodes.add(code_point)
        except Exception as e:
            file_path = str(file) if hasattr(file, '_path') else file
            print("Error reading {}: {}".format(file_path, e))
    
    return sorted(unicodes)

def main():
    src_dir_str = str(SRC_DIR) if hasattr(SRC_DIR, '_path') else SRC_DIR
    print("Scanning for .slint files in: {}".format(src_dir_str))
    slint_files = get_all_slint_files(SRC_DIR)
    print("Found {} files.".format(len(slint_files)))
    
    print("Extracting used icons...")
    unicodes = extract_unicodes(slint_files)
    print("Found {} unique icons:".format(len(unicodes)))
    for code in unicodes:
        print("  U+{:04X}".format(code))
    
    if not unicodes:
        print("No icons found! Aborting subsetting to avoid empty font.")
        return
    
    font_source_str = str(FONT_SOURCE) if hasattr(FONT_SOURCE, '_path') else FONT_SOURCE
    output_file_str = str(OUTPUT_FILE) if hasattr(OUTPUT_FILE, '_path') else OUTPUT_FILE
    print("\nSubsetting font from {} to {}...".format(font_source_str, output_file_str))
    
    font_source_path = str(FONT_SOURCE) if hasattr(FONT_SOURCE, '_path') else FONT_SOURCE
    if not os.path.exists(font_source_path):
        print("Error: Source font not found at {}".format(font_source_str))
        return
    
    try:
        # Load the font
        font_source_path = str(FONT_SOURCE) if hasattr(FONT_SOURCE, '_path') else FONT_SOURCE
        font = TTFont(font_source_path)
        
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
        output_file_path = str(OUTPUT_FILE) if hasattr(OUTPUT_FILE, '_path') else OUTPUT_FILE
        font.save(output_file_path)
        
        # Get file sizes
        font_source_path = str(FONT_SOURCE) if hasattr(FONT_SOURCE, '_path') else FONT_SOURCE
        output_file_path = str(OUTPUT_FILE) if hasattr(OUTPUT_FILE, '_path') else OUTPUT_FILE
        original_size = os.path.getsize(font_source_path)
        new_size = os.path.getsize(output_file_path)
        
        output_file_str = str(OUTPUT_FILE) if hasattr(OUTPUT_FILE, '_path') else OUTPUT_FILE
        print("\nSuccessfully generated: {}".format(output_file_str))
        print("Original size: {:.2f} KB".format(original_size / 1024.0))
        print("New size: {:.2f} KB".format(new_size / 1024.0))
        print("Reduction: {:.2f}%".format((1 - new_size / float(original_size)) * 100))
        
    except Exception as e:
        print("Error during font subsetting: {}".format(e))
        import traceback
        traceback.print_exc()

if __name__ == "__main__":
    main()
