$ErrorActionPreference = "Stop"

$SCRIPT_DIR = Split-Path -Parent $MyInvocation.MyCommand.Definition
$JS_SCRIPT = Join-Path $SCRIPT_DIR "..\..\assets\font\generate_subset.js"
$PY_SCRIPT = Join-Path $SCRIPT_DIR "..\..\assets\font\generate_subset.py"
# Resolve project root (two levels up from build/scripts)
$PROJECT_ROOT = Resolve-Path (Join-Path $SCRIPT_DIR "..\..")

Write-Host "Starting font subsetting process..." -ForegroundColor Cyan

# Check for Node.js and npm
$hasNode = Get-Command node -ErrorAction SilentlyContinue
$hasNpm = Get-Command npm -ErrorAction SilentlyContinue

if ($hasNode -and $hasNpm) {
    Write-Host "Node.js detected. Using JavaScript implementation..." -ForegroundColor Green
    
    # Check for fontmin dependency
    Write-Host "Checking for fontmin dependency..."
    $fontminInstalled = $false

    Push-Location $PROJECT_ROOT
    try {
        # Direct execution avoids Start-Process argument parsing headaches
        node -e "try { require('fontmin') } catch (e) { process.exit(1) }"
        if ($LASTEXITCODE -eq 0) {
            $fontminInstalled = $true
        }
    }
    finally {
        Pop-Location
    }

    if (-not $fontminInstalled) {
        Write-Host "Error: 'fontmin' dependency is missing." -ForegroundColor Red
        Write-Host "To fix this:" -ForegroundColor Yellow
        Write-Host "1. Open PowerShell or CMD as Administrator"
        Write-Host "2. Navigate to the project root: $PROJECT_ROOT"
        Write-Host "3. Run command: npm install fontmin --save-dev" -ForegroundColor White
        exit 1
    }

    Write-Host "Dependency check passed. Running subset_font.js..."
    node $JS_SCRIPT

    if ($LASTEXITCODE -eq 0) {
        Write-Host "Font subsetting finished successfully." -ForegroundColor Green
    }
    else {
        Write-Host "Font subsetting failed." -ForegroundColor Red
        exit 1
    }
}
else {
    Write-Host "Node.js not found. Checking for Python..." -ForegroundColor Yellow
    
    # Check for Python
    $hasPython = Get-Command python -ErrorAction SilentlyContinue
    if (-not $hasPython) {
        $hasPython = Get-Command python3 -ErrorAction SilentlyContinue
    }
    
    if (-not $hasPython) {
        Write-Host "Error: Neither Node.js nor Python found." -ForegroundColor Red
        Write-Host "Please install either:" -ForegroundColor Yellow
        Write-Host "  - Node.js from https://nodejs.org/" -ForegroundColor White
        Write-Host "  - Python from https://www.python.org/" -ForegroundColor White
        exit 1
    }
    
    Write-Host "Python detected. Using Python implementation..." -ForegroundColor Green
    
    # Determine the correct python command to use
    $pythonCmd = if ($hasPython.Name -eq "python3.exe") { "python3" } else { "python" }
    
    # Check for fonttools dependency
    Write-Host "Checking for fonttools dependency..."
    
    Push-Location $PROJECT_ROOT
    try {
        & $pythonCmd -c "import fontTools" 2>$null
        if ($LASTEXITCODE -ne 0) {
            Write-Host "Error: 'fonttools' package is missing." -ForegroundColor Red
            Write-Host "To fix this, run:" -ForegroundColor Yellow
            Write-Host "  $pythonCmd -m pip install fonttools" -ForegroundColor White
            exit 1
        }
    }
    finally {
        Pop-Location
    }
    
    Write-Host "Dependency check passed. Running generate_subset.py..."
    & $pythonCmd $PY_SCRIPT
    
    if ($LASTEXITCODE -eq 0) {
        Write-Host "Font subsetting finished successfully." -ForegroundColor Green
    }
    else {
        Write-Host "Font subsetting failed." -ForegroundColor Red
        exit 1
    }
}
