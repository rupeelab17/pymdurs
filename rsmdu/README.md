# pymdurs

Rust transpilation of [pymdu](https://github.com/rupeelab17/pymdu) (Python Urban Data Model).

This project transpiles the Python pymdu library to Rust, using the [GeoRust](https://georust.org/) ecosystem for geospatial operations. It provides three deployment targets: native Rust library, Python bindings (PyO3), and WebAssembly (WASM) for browser-based applications.

## 🎯 Project Overview

**pymdurs** is a comprehensive geospatial data processing library for urban data analysis, providing:

- **Native Rust library** (`rsmdu`) - High-performance geospatial operations
- **Python bindings** (`pymdurs`) - Pythonic API with PyO3
- **WebAssembly bindings** (`rsmdu-wasm`) - Browser-based geospatial processing

## 📦 Project Structure

This project is organized as a **Cargo workspace** with three main crates:

```
pymdurs/
├── rsmdu/              # Core Rust library
│   ├── src/
│   │   ├── geometric/          # Geometric data structures
│   │   │   ├── building.rs     # Building and BuildingCollection
│   │   │   ├── dem.rs          # Digital Elevation Model
│   │   │   ├── cadastre.rs     # Cadastral parcel data
│   │   │   ├── iris.rs         # IRIS statistical units
│   │   │   ├── lcz.rs          # Local Climate Zone
│   │   │   ├── road.rs         # Road segments
│   │   │   ├── water.rs        # Water bodies
│   │   │   └── vegetation.rs   # Vegetation zones
│   │   ├── collect/            # Data collection modules
│   │   │   ├── ign/            # IGN API integration
│   │   │   │   └── ign_collect.rs
│   │   │   └── global_variables.rs
│   │   ├── geo_core.rs         # Base GeoCore class
│   │   ├── commons/            # Common utilities
│   │   ├── lib.rs              # Library root
│   │   └── main.rs             # Binary entry point
│   └── examples/               # Rust usage examples
│
├── pymdurs/            # Python bindings (PyO3)
│   ├── src/
│   │   └── bindings/          # PyO3 bindings
│   ├── examples/              # Python usage examples
│   └── tests/                 # Python tests
│
└── rsmdu-wasm/        # WebAssembly bindings
    ├── src/
    │   └── lib.rs             # WASM bindings
    └── examples/              # Browser examples
        ├── index.html         # Building visualization
        └── dem.html           # DEM visualization
```

## ✨ Features

### 🏢 Building Data Management

- **Building data collection**: Load buildings from Shapefiles, GeoJSON, or IGN API
- **Geometric operations**: Area, centroid, and height calculations
- **Data processing**: Height processing with fallback to storeys or mean district height
- **Height calculation**: Automatic height filling using:
  - Number of storeys × default storey height
  - Alternative height field (HAUTEUR_2)
  - Mean district height (weighted by area)
- **DataFrame support**: Integration with Polars for tabular operations (equivalent to GeoDataFrame)
- **WebAssembly support**: Process building data directly in the browser

### 🗻 Digital Elevation Model (DEM)

- **DEM collection**: Download DEM data from IGN API via WMS-R
- **Raster processing**: GeoTIFF handling and validation
- **Mask generation**: Automatic mask creation for DEM boundaries
- **Browser visualization**: Interactive DEM viewer with Leaflet.js integration
- **IGN API integration**: Direct loading from IGN WMS service in browser

### 🗺️ Cadastral Data

- **Cadastre collection**: Download cadastral parcel data from IGN API via WFS
- **GeoJSON parsing**: Automatic parsing of IGN API responses
- **GeoJSON export**: Save cadastral data to GeoPackage format

### 📊 IRIS Statistical Units

- **IRIS collection**: Download IRIS (statistical units) data from IGN API via WFS
- **GeoJSON parsing**: Automatic parsing of IGN API responses
- **GeoJSON export**: Save IRIS data to GeoPackage format

### 🌡️ LCZ (Local Climate Zone)

- **LCZ collection**: Load Local Climate Zone data from external sources
- **Color mapping**: Built-in LCZ color table (17 LCZ types)
- **Spatial filtering**: Filter LCZ data by bounding box
- **Shapefile support**: Load from zip URLs (requires GDAL)

### 🛣️ Road and Infrastructure

- **Road collection**: Download road segments from IGN API
- **Water bodies**: Download water body data from IGN API
- **Vegetation zones**: Download vegetation data from IGN API

### 🛰️ LiDAR Processing

- **LiDAR data collection**: Download LAZ files from IGN WFS service
- **Point cloud processing**: Load and process LiDAR point clouds
- **Raster generation**: Create DSM (Digital Surface Model), DTM (Digital Terrain Model), and CHM (Canopy Height Model)
- **Classification filtering**: Filter points by classification (ground, vegetation, buildings, water, etc.)
- **Multi-band GeoTIFF export**: Export processed rasters as multi-band GeoTIFF files
- **Automatic workflow**: Points are automatically fetched and loaded when bounding box is set

### 🌐 IGN API Integration

- **WFS (Web Feature Service)**: Vector data retrieval (buildings, roads, water, etc.)
- **WMS (Web Map Service)**: Raster data retrieval (DEM, orthoimagery, etc.)
- **WMS-R endpoint**: Optimized raster service endpoint
- **OGC compliance**: Follows OGC WFS 2.0.0 and WMS 1.3.0 standards
- **Browser support**: Direct API calls from WebAssembly

### 🎨 WebAssembly Features

- **Building processing**: Load and process building GeoJSON in the browser
- **DEM visualization**: Interactive DEM viewer with color scales
- **IGN API integration**: Fetch data directly from IGN API in browser
- **Leaflet.js integration**: Seamless integration with Leaflet maps
- **Dynamic bbox**: Automatic bounding box updates based on map view

## 🚀 Installation

### Python Package

**Requirements:** Python >= 3.10, pandas >= 2.0.0, numpy >= 2.0.2

**Important:** Run maturin from the **project root** (where `pyproject.toml` is). Maturin uses `manifest-path = "pymdurs/Cargo.toml"`.

#### 1. Clone and setup

```bash
git clone https://github.com/rupeelab17/rsmdu.git
cd rsmdu
uv venv .venv --python 3.13
source .venv/bin/activate
uv pip install maturin
uv sync
```

#### 2. Install GDAL (prerequisite)

| Platform    | Command                                                                                                                                                            |
| ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **macOS**   | `brew install gdal` or `ARCHFLAGS="-arch arm64" uv pip install --no-cache-dir gdal`                                                                                |
| **Linux**   | `sudo apt-get update && sudo apt-get install -y libgdal-dev gdal-bin libclang-dev`                                                                                 |
| **Windows** | [OSGeo4W](https://trac.osgeo.org/osgeo4w/) (GDAL, GEOS, PROJ, SQLite3) + `choco install llvm pkgconfiglite sqlite -y` + set `GDAL_HOME`, `PKG_CONFIG_PATH`, `PATH` |

#### 3. Build (maturin develop)

| Platform                  | Target                      | Command                                                                  |
| ------------------------- | --------------------------- | ------------------------------------------------------------------------ |
| **macOS** (Apple Silicon) | `aarch64-apple-darwin`      | `maturin develop --target aarch64-apple-darwin`                          |
| **macOS** (Intel)         | `x86_64-apple-darwin`       | `maturin develop --target x86_64-apple-darwin`                           |
| **Linux** (x86_64)        | `x86_64-unknown-linux-gnu`  | `maturin develop --target x86_64-unknown-linux-gnu` or `maturin develop` |
| **Linux** (ARM64)         | `aarch64-unknown-linux-gnu` | `maturin develop --target aarch64-unknown-linux-gnu`                     |
| **Windows**               | `x86_64-pc-windows-msvc`    | `maturin develop --target x86_64-pc-windows-msvc`                        |

With uv: `uv run maturin develop --target <target>`

**macOS: linker can't find GDAL / "library 'gdal' not found"**  
If you upgraded GDAL or PROJ with Homebrew, the linker may still use old paths. Clean and point pkg-config at the current install, then rebuild:

```bash
cd pymdurs && cargo clean && cd ..
export PKG_CONFIG_PATH="/opt/homebrew/lib/pkgconfig"
maturin develop --target aarch64-apple-darwin
```

#### 4. Verify and release

```bash
python -c "import pymdurs; print('OK')"
# Release wheel
maturin build --target <target> --release
```

### Installing Rust

Maturin compiles the Rust extension. If Rust is not installed, follow the instructions for your operating system:

#### Windows

1. **Download and run rustup-init.exe:**
   - Visit https://rustup.rs/
   - Download `rustup-init.exe`
   - Run the installer and follow the prompts

2. **Or use PowerShell:**

   ```powershell
   Invoke-WebRequest -Uri https://win.rustup.rs/x86_64 -OutFile rustup-init.exe
   .\rustup-init.exe
   ```

3. **Verify installation:**

   ```powershell
   rustc --version
   cargo --version
   ```

4. **Install Visual Studio Build Tools (required for Windows):**
   - Download from: https://visualstudio.microsoft.com/downloads/
   - Install "Desktop development with C++" workload
   - Or install the lighter "C++ build tools": https://visualstudio.microsoft.com/visual-cpp-build-tools/

**Note**: On Windows, you may need to restart your terminal after installation for PATH changes to take effect.

#### macOS

1. **Install using Homebrew (recommended):**

   ```bash
   brew install rust
   ```

2. **Or use rustup (official installer):**

   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

3. **Follow the prompts** and restart your terminal

4. **Verify installation:**
   ```bash
   rustc --version
   cargo --version
   ```

**Note**: On macOS, you may need to install Xcode Command Line Tools:

```bash
xcode-select --install
```

#### Linux

1. **Install using your package manager:**

   **Ubuntu/Debian:**

   ```bash
   sudo apt update
   sudo apt install rustc cargo
   ```

   **Fedora:**

   ```bash
   sudo dnf install rust cargo
   ```

   **Arch Linux:**

   ```bash
   sudo pacman -S rust
   ```

2. **Or use rustup (recommended for latest version):**

   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source $HOME/.cargo/env
   ```

3. **Install build dependencies (required):**

   **Ubuntu/Debian:**

   ```bash
   sudo apt install build-essential pkg-config libssl-dev
   ```

   **Fedora:**

   ```bash
   sudo dnf install gcc pkg-config openssl-devel
   ```

   **Arch Linux:**

   ```bash
   sudo pacman -S base-devel
   ```

4. **Verify installation:**
   ```bash
   rustc --version
   cargo --version
   ```

#### Docker

When building an image that installs pymdurs with `uv pip install https://github.com/rupeelab17/rsmdu.git`, **Rust must be installed in a previous layer**. If Rust is not present, maturin installs it on the fly and can hit "Text file busy (os error 26)" when running `cargo metadata` immediately after.

**Fix:** install Rust (e.g. with rustup) **before** the step that runs `uv pip install`:

```dockerfile
# Install Rust so maturin does not auto-install it (avoids "Text file busy")
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable \
    && . "$HOME/.cargo/env" \
    && rustc --version
ENV PATH="/root/.cargo/bin:${PATH}"

# Then install pymdurs (maturin will find cargo and build the wheel)
RUN micromamba run -n umep_pymdu python -m uv pip install https://github.com/rupeelab17/rsmdu.git
```

Adjust the environment (e.g. `micromamba run -n umep_pymdu`, or `ENV PATH` if Rust is installed for another user) to match your image.

### WebAssembly

```bash
# Install wasm-pack
cargo install wasm-pack

# Add WASM target
rustup target add wasm32-unknown-unknown

# Navigate to WASM package directory
cd rsmdu-wasm

# Fix WASM target (if needed)
./fix-wasm-target.sh

# Build WASM package
./build.sh
```

See `rsmdu-wasm/README.md` for detailed WebAssembly setup instructions.

## 📖 Usage

### Rust Library

#### Building Collection

```rust
use rsmdu::geometric::building::BuildingCollection;

// Create BuildingCollection (Python: Building(output_path='./'))
let mut buildings = BuildingCollection::new(
    None,                              // filepath_shp (None = use IGN API)
    Some("./output".to_string()),      // output_path
    3.0,                               // defaultStoreyHeight
    None,                              // set_crs (None = default EPSG:2154)
)?;

// Set bounding box (Python: buildings.bbox = [...])
buildings.set_bbox(-1.152704, 46.181627, -1.139893, 46.18699)?;

// Run processing (Python: buildings = buildings.run())
let buildings = buildings.run()?;

// Convert to Polars DataFrame (Python: buildings.to_gdf())
let df = buildings.to_polars_df()?;
```

#### DEM (Digital Elevation Model)

```rust
use rsmdu::geometric::dem::Dem;

// Create Dem instance
let mut dem = Dem::new(Some("./output".to_string()))?;

// Set bounding box
dem.set_bbox(-1.152704, 46.181627, -1.139893, 46.18699);

// Run DEM processing
let dem_result = dem.run(None)?;

// Get output paths
println!("DEM saved to: {:?}", dem_result.get_path_save_tiff());
println!("Mask: {:?}", dem_result.get_path_save_mask());
```

### Python Bindings

```python
import pymdurs

# Create BuildingCollection
buildings = pymdurs.geometric.Building(
    output_path="./output",
    defaultStoreyHeight=3.0
)

# Set bounding box
buildings.set_bbox(-1.152704, 46.181627, -1.139893, 46.18699)

# Run processing (downloads from IGN API)
buildings = buildings.run()

# Convert to pandas DataFrame
df = buildings.to_pandas()
```

#### LiDAR Processing

```python
import pymdurs

# Create Lidar instance
lidar = pymdurs.geometric.Lidar(output_path="./output")

# Set bounding box (automatically fetches LAZ file URLs and loads points)
# Format: min_x, min_y, max_x, max_y (WGS84, EPSG:4326)
lidar.set_bbox(-1.154894, 46.182639, -1.148361, 46.186820)

# Set CRS (optional, defaults to EPSG:2154)
lidar.set_crs(2154)

# Process points to create rasters (DSM, DTM, CHM)
# Classification filter: 2=Ground, 3=Low Vegetation, 4=Medium, 5=High, 6=Building, 9=Water
classification_list = [3, 4, 5, 9]  # Vegetation and water
output_path = lidar.run(file_name="CDSM.tif", classification_list=classification_list)

print(f"GeoTIFF saved to: {output_path}")
# File contains 3 bands: DSM, DTM, CHM
```

### WebAssembly (Browser)

#### Building Processing

```javascript
import init, { WasmBuildingCollection } from "./pkg/rsmdu_wasm.js";

// Initialize WASM
await init("./pkg/rsmdu_wasm_bg.wasm");

// Load buildings from GeoJSON
const geojson = {
  type: "FeatureCollection",
  features: [
    /* ... */
  ],
};

const collection = WasmBuildingCollection.from_geojson(
  JSON.stringify(geojson),
  3.0, // default storey height in meters
);

// Process heights
collection.process_heights();

// Get processed GeoJSON
const processedGeojson = collection.to_geojson();

// Get statistics
const stats = collection.get_stats();
console.log("Building count:", stats.count);
console.log("Total area:", stats.total_area);
console.log("Mean height:", stats.mean_height);
```

#### DEM Loading

```javascript
import init, { WasmDem } from "./pkg/rsmdu_wasm.js";

// Initialize WASM
await init("./pkg/rsmdu_wasm_bg.wasm");

// Load DEM from IGN API
const dem = await WasmDem.from_ign_api(
  -1.152704, // min_x
  46.181627, // min_y
  -1.139893, // max_x
  46.18699, // max_y
);

// Get DEM dimensions
console.log("Width:", dem.width());
console.log("Height:", dem.height());

// Get extent
const extent = dem.get_extent();
console.log("Extent:", extent);
```

See `rsmdu-wasm/examples/index.html` and `rsmdu-wasm/examples/dem.html` for complete browser examples.

## 📚 Examples

Comprehensive examples are available for all three deployment targets:

### Rust Examples

Located in `rsmdu/examples/`:

**Building Examples:**

- **`building_manual.rs`**: Minimal example of manually creating buildings
- **`building_from_geojson.rs`**: Complete example loading from GeoJSON
- **`building_from_ign.rs`**: Example using `run()` method (Python-style)
- **`building_from_ign_api.rs`**: Detailed example loading buildings from IGN API
- **`building_complete.rs`**: Comprehensive example covering all use cases

**Other Examples:**

- **`dem_from_ign.rs`**: Downloading and processing DEM from IGN API
- **`cadastre_from_ign.rs`**: Downloading and processing cadastral data from IGN API
- **`iris_from_ign.rs`**: Downloading and processing IRIS statistical units from IGN API
- **`lcz_from_url.rs`**: Loading and processing LCZ data from URL
- **`cosia_from_ign.rs`**: Downloading COSIA landcover raster from IGN API
- **`rnb_from_api.rs`**: Downloading RNB (French National Building Reference) data from RNB API
- **`road_from_ign.rs`**: Downloading road segments from IGN API
- **`water_from_ign.rs`**: Downloading water bodies from IGN API
- **`vegetation_from_ign.rs`**: Downloading vegetation zones from IGN API

**Run Rust examples:**

```bash
cd rsmdu
cargo run --example building_manual
cargo run --example building_from_geojson
cargo run --example building_from_ign
cargo run --example dem_from_ign
cargo run --example cadastre_from_ign
cargo run --example iris_from_ign
cargo run --example lcz_from_url
cargo run --example cosia_from_ign
cargo run --example rnb_from_api
```

### Python Examples

Located in `examples/` (see [examples/README.md](examples/README.md) for full documentation):

- **`building_basic.py`**: Basic Building usage
- **`building_from_ign.py`**: Load buildings from IGN API
- **`dem_from_ign.py`**: Download DEM from IGN API
- **`cadastre_from_ign.py`**: Download cadastral data from IGN API
- **`iris_from_ign.py`**: Download IRIS statistical units from IGN API
- **`lcz_from_url.py`**: Load LCZ data from URL
- **`cosia_from_ign.py`**: Complete COSIA workflow - download, vectorize and convert to UMEP format
- **`rnb_from_api.py`**: Download RNB (French National Building Reference) data from RNB API
- **`lidar_from_wfs.py`**: Download and process LiDAR data from IGN WFS service
- **`road_from_ign.py`**: Download road segments from IGN API
- **`water_from_ign.py`**: Download water bodies from IGN API
- **`vegetation_from_ign.py`**: Download vegetation zones from IGN API
- **`umep_workflow.py`**: Complete UMEP workflow for urban climate modeling (SOLWEIG, SVF, etc.)
- **`umep_workflow_new.py`**: Alternative UMEP workflow using solweig (SOLWEIG from UMEP-dev), with GIF export from preview PNGs

**Run Python examples** (from project root):

```bash
python examples/building_basic.py
python examples/building_from_ign.py
python examples/dem_from_ign.py
python examples/cosia_from_ign.py
python examples/umep_workflow.py
python examples/umep_workflow_new.py
```

### WebAssembly Examples

Located in `rsmdu-wasm/examples/`:

- **`index.html`**: Interactive building visualization with Leaflet.js
  - Load buildings from GeoJSON or IGN API
  - Dynamic bounding box based on map view
  - Building statistics and visualization
- **`dem.html`**: Interactive DEM visualization with Leaflet.js
  - Load DEM from local TIFF files or IGN API
  - Multiple color scales (terrain, elevation, grayscale)
  - Elevation query on click
  - Automatic OSM layer opacity adjustment

**Run WebAssembly examples:**

```bash
cd rsmdu-wasm
./build.sh
cd examples
python3 -m http.server 8000
# Open http://localhost:8000/index.html or http://localhost:8000/dem.html
```

## 🌐 IGN API

The library integrates with the French IGN (Institut Géographique National) Géoplateforme API.

### Available Services

**Vector Data (WFS)**:

- `buildings`: Building footprints (BDTOPO_V3:batiment)
- `road`: Road segments (BDTOPO_V3:troncon_de_route)
- `water`: Water bodies (BDTOPO_V3:plan_d_eau)
- `cadastre`: Cadastral parcels (CADASTRALPARCELS.PARCELLAIRE_EXPRESS:parcelle)
- `iris`: IRIS statistical units (STATISTICALUNITS.IRIS:contours_iris)
- `lidar`: LiDAR point cloud data (IGNF_LIDAR-HD_TA:nuage-dalle)
- `vegetation`: Vegetation zones
- `hydrographique`: Hydrographic details

**Raster Data (WMS-R)**:

- `dem`: Digital Elevation Model (ELEVATION.ELEVATIONGRIDCOVERAGE.HIGHRES)
- `dsm`: Digital Surface Model
- `irc`: IRC orthoimagery
- `ortho`: High-resolution orthoimagery
- `cosia`: COSIA imagery

### API Requirements

- **Internet connection**: Required for API requests
- **Rate limiting**: The API may have rate limits
- **Coordinate system**: Bounding boxes must be in WGS84 (EPSG:4326)
- **API keys**: For production use, you may need to register at https://geoservices.ign.fr/
- **CORS**: Browser-based requests require CORS support (available for IGN API)

## 🏗️ Architecture

### Workspace Structure

The project uses a Cargo workspace with three crates:

1. **`rsmdu`**: Core Rust library with all geospatial functionality
   - Uses feature flags (`wasm`) to conditionally compile WASM-incompatible dependencies
   - Optional dependencies: `gdal`, `proj`, `geos`, `polars`, `reqwest`, etc.

2. **`pymdurs`**: Python bindings using PyO3
   - Depends on `rsmdu` crate
   - Provides Pythonic API with aliases

3. **`rsmdu-wasm`**: WebAssembly bindings
   - Uses `rsmdu` with `wasm` feature flag
   - Only WASM-compatible dependencies (geo, geojson, geotiff, tiff)
   - Browser-specific APIs (web-sys, wasm-bindgen)

### Inheritance Pattern (Python → Rust)

In Python, `Building` inherits from `IgnCollect`, which inherits from `GeoCore`. In Rust, we use composition:

```rust
// Python: class Building(IgnCollect)
// Rust: BuildingCollection contains GeoCore and IgnCollect
pub struct BuildingCollection {
    pub geo_core: GeoCore,           // Inherits from GeoCore
    ign_collect: Option<IgnCollect>,  // Inherits from IgnCollect
    // ...
}
```

### GeoCore Properties

`GeoCore` provides base functionality:

- `epsg`: Coordinate Reference System (default: 2154)
- `bbox`: Bounding box
- `output_path`: Output directory
- `output_path_shp`: Shapefile output path
- `filename_shp`: Shapefile filename

## 📊 Status

### ✅ Fully Implemented

**Core Features:**

- ✅ GeoJSON parsing and building collection
- ✅ IGN API integration (WFS and WMS)
- ✅ Building height processing with multiple fallback strategies
- ✅ DEM collection via WMS-R
- ✅ Cadastral data collection via WFS
- ✅ IRIS statistical units collection via WFS
- ✅ LCZ data structure with color mapping (17 LCZ types)
- ✅ Road, water, and vegetation data collection
- ✅ Polars DataFrame conversion
- ✅ GeoCore base class with all properties
- ✅ BuildingCollection with `run()` method (Python-style)
- ✅ Coordinate Reference System (CRS) transformations using Proj

**Python Bindings (PyO3):**

- ✅ Complete Python bindings installable via `pip install pymdurs`
- ✅ All geometric classes available: `Building`, `Dem`, `Cadastre`, `Iris`, `Lcz`, `Lidar`, `Road`, `Water`, `Vegetation`
- ✅ Geometric submodule (`pymdurs.geometric`) for organized class access
- ✅ Pythonic API with aliases (e.g., `pymdurs.geometric.Building` instead of `PyBuilding`)
- ✅ Pandas DataFrame conversion for Building data
- ✅ GeoJSON export/import
- ✅ LiDAR processing with automatic point loading
- ✅ Comprehensive Python examples and tests

**WebAssembly Bindings:**

- ✅ Building collection processing in browser
- ✅ DEM loading from IGN API in browser
- ✅ DEM loading from local TIFF files
- ✅ Interactive Leaflet.js visualization
- ✅ Dynamic bounding box updates
- ✅ Building statistics
- ✅ DEM visualization with color scales
- ✅ Elevation query on map click
- ✅ OSM layer opacity control

**Data Formats:**

- ✅ GeoJSON parsing and generation
- ✅ GeoTIFF reading and validation
- ✅ GeoJSON export (simplified, saves as GeoJSON temporarily)

### 🚧 In Progress / Limitations

**Current Limitations:**

- ⚠️ GDAL Shapefile I/O temporarily disabled (API compatibility issues)
- ⚠️ Full GeoJSON export not yet implemented (saves as GeoJSON as workaround)
- ⚠️ LCZ shapefile loading from zip URLs requires full GDAL implementation
- ⚠️ Raster reprojection simplified (full resampling not yet implemented)
- ⚠️ Shapefile export for masks not yet available
- ⚠️ DEM pixel data access in WASM (currently metadata only)

**Workarounds:**

- Use GeoJSON format instead of Shapefiles for input/output
- GeoJSON export currently saves as GeoJSON (full GeoJSON support planned)
- LCZ processing structure ready but full shapefile loading pending GDAL fixes
- DEM visualization uses JavaScript libraries (GeoRasterLayer) for pixel access

### 📋 Planned Features

**Short-term:**

- Complete GDAL integration for Shapefile I/O
- Full GeoJSON export with proper layer management
- Full raster reprojection with resampling options
- LCZ shapefile loading from zip URLs
- Full DEM pixel data access in WASM

**Long-term:**

- Additional geometric operations
- More IGN API services
- Performance optimizations
- Additional output formats
- Enhanced error handling and validation
- 3D visualization capabilities

## 🧪 Testing

### Rust Tests

Run Rust unit tests:

```bash
cd rsmdu
cargo test
```

### Python Tests

Run Python tests:

```bash
cd pymdurs
pytest tests/
```

Or manually test the Python bindings:

```bash
cd pymdurs
python -c "import pymdurs; print('✅ pymdurs imported successfully')"
python -c "import pymdurs; print('Available classes:', [x for x in dir(pymdurs.geometric) if not x.startswith('_')])"
python -c "import pymdurs; lidar = pymdurs.geometric.Lidar(output_path='./test'); print('✅ Lidar class works')"
```

### WebAssembly Tests

```bash
cd rsmdu-wasm
wasm-pack test --headless --firefox
```

## 🤝 Contributing

Contributions are welcome! This project follows standard Rust and Python best practices.

### Development Setup

1. **Clone the repository:**

   ```bash
   git clone https://github.com/rupeelab17/pymdurs.git
   cd pymdurs
   ```

2. **Set up Rust development:**

   **Note**: If you haven't installed Rust yet, see the [Installing Rust](#installing-rust) section above.

   ```bash
   cd rsmdu
   cargo build
   cargo test
   ```

3. **Set up Python development:** See [Python Package](#python-package) for platform-specific build instructions.

4. **Set up WebAssembly development:**
   ```bash
   cd rsmdu-wasm
   rustup target add wasm32-unknown-unknown
   cargo install wasm-pack
   ./build.sh
   ```

### Contribution Guidelines

- Follow Rust naming conventions and style
- Add tests for new features
- Update documentation (README.md) for significant changes
- Ensure all examples still work after changes
- Test all three deployment targets (Rust, Python, WASM)
- Use feature flags for WASM-incompatible code

### Reporting Issues

Please use GitHub Issues to report bugs or request features. Include:

- Description of the issue
- Steps to reproduce
- Expected vs actual behavior
- Environment details (OS, Rust version, Python version, browser for WASM)

## 📄 License

GPL-3.0 (same as [pymdu](https://github.com/rupeelab17/pymdu))

## ⚡ Performance

This Rust implementation provides significant performance improvements over the original Python version:

- **Memory efficiency**: Rust's ownership system prevents memory leaks
- **Concurrency**: Safe parallel processing capabilities
- **Speed**: Native compilation provides faster execution
- **Type safety**: Compile-time error checking reduces runtime errors
- **WebAssembly**: Near-native performance in the browser

## 🔗 Related Projects

- [pymdu](https://github.com/rupeelab17/pymdu): Original Python implementation
- [GeoRust](https://georust.org/): Rust geospatial ecosystem
- [IGN Géoplateforme](https://geoservices.ign.fr/): French geospatial data services
- [PyO3](https://pyo3.rs/): Rust bindings for Python
- [Maturin](https://github.com/PyO3/maturin): Build tool for Python packages with Rust extensions
- [wasm-pack](https://rustwasm.github.io/wasm-pack/): Build tool for WebAssembly packages

## 📝 Version

Current version: **0.1.1** (Alpha)

This is an early release. The API may change in future versions. See the [Status](#-status) section for implementation details.

## 📚 Documentation

- **Rust API**: Run `cargo doc --open` in the `rsmdu` directory
- **Python API**: See `pymdurs/README.md`
- **WebAssembly API**: See `rsmdu-wasm/README.md`
- **Examples**: See [examples/README.md](examples/README.md), `rsmdu/examples/`, and `rsmdu-wasm/examples/`
