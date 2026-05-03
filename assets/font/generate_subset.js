// detect-and-subset.js
const fs = require('fs');
const path = require('path');
const Fontmin = require('fontmin');

// Configuration
const SRC_DIR = path.resolve(__dirname, '../../src');
const FONT_SOURCE = path.resolve(__dirname, 'segoeicons.ttf');
const OUTPUT_DIR = path.resolve(__dirname);
const OUTPUT_FILE = 'icons.ttf';

if (!fs.existsSync(FONT_SOURCE)) {
    console.error(`Error: Source font not found at ${FONT_SOURCE}`);
    process.exit(1);
}

// 1. Recursive file search to find all .slint files
function getAllFiles(dirPath, arrayOfFiles) {
    const files = fs.readdirSync(dirPath);

    arrayOfFiles = arrayOfFiles || [];

    files.forEach(function (file) {
        if (fs.statSync(dirPath + "/" + file).isDirectory()) {
            arrayOfFiles = getAllFiles(dirPath + "/" + file, arrayOfFiles);
        } else {
            if (file.endsWith('.slint')) {
                arrayOfFiles.push(path.join(dirPath, "/", file));
            }
        }
    });

    return arrayOfFiles;
}

// 2. Extract unicode characters (\u{XXXX} or just raw characters if any)
// Assuming format like "\u{E700}" in .slint files.
function extractUnicodes(files) {
    let unicodes = new Set();
    // Regex for standard unicode escape sequences in Slint strings: \u{XXXX}
    const regex = /\\u\{([0-9a-fA-F]+)\}/g;

    files.forEach(file => {
        const content = fs.readFileSync(file, 'utf8');
        let match;
        while ((match = regex.exec(content)) !== null) {
            // match[1] is the hex code, e.g., "e700"
            // We need to convert it to the actual character
            const char = String.fromCodePoint(parseInt(match[1], 16));
            unicodes.add(char);
        }
    });

    return Array.from(unicodes).join('');
}

console.log('Scanning for .slint files in:', SRC_DIR);
const slintFiles = getAllFiles(SRC_DIR);
console.log(`Found ${slintFiles.length} files.`);

console.log('Extracting used icons...');
const text = extractUnicodes(slintFiles);
console.log(`Found ${text.length} unique characters:`, text.split('').map(c => '\\u' + c.codePointAt(0).toString(16).toUpperCase()).join(' '));

if (text.length === 0) {
    console.warn("No icons found! Aborting subsetting to avoid empty font.");
    process.exit(0);
}

// 3. Use Fontmin to subset
console.log(`Subsetting font from ${FONT_SOURCE} to ${OUTPUT_FILE}...`);

const TEMP_DIR = path.resolve(__dirname, '../../build/temp_font');

// Create temp dir if not exists
if (!fs.existsSync(TEMP_DIR)) {
    fs.mkdirSync(TEMP_DIR, { recursive: true });
}

const fontmin = new Fontmin()
    .src(FONT_SOURCE)
    .use(Fontmin.glyph({
        text: text,
        hinting: false
    }))
    .dest(TEMP_DIR);

fontmin.run(function (err, files) {
    if (err) {
        console.error('Error during font subsetting:', err);
        process.exit(1);
    }

    const generatedFile = path.join(TEMP_DIR, 'segoeicons.ttf');
    const targetFile = path.join(OUTPUT_DIR, OUTPUT_FILE);

    if (fs.existsSync(generatedFile)) {
        try {
            if (fs.existsSync(targetFile)) {
                fs.unlinkSync(targetFile);
            }
            // Move file from temp to target
            fs.copyFileSync(generatedFile, targetFile);
            console.log(`Successfully generated: ${targetFile}`);

            // Cleanup temp
            fs.unlinkSync(generatedFile);
            fs.rmdirSync(TEMP_DIR);

            // Log file size diff
            const originalStats = fs.statSync(FONT_SOURCE);
            const newStats = fs.statSync(targetFile);
            console.log(`Original size: ${(originalStats.size / 1024).toFixed(2)} KB`);
            console.log(`New size: ${(newStats.size / 1024).toFixed(2)} KB`);
            console.log(`Reduction: ${((1 - newStats.size / originalStats.size) * 100).toFixed(2)}%`);

        } catch (e) {
            console.error('Error moving file:', e);
        }
    } else {
        console.error('Expected output file not found in temp:', generatedFile);
    }
});
