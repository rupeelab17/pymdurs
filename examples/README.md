# pymdurs usage examples

This folder contains Python examples for using the `pymdurs` package to collect and process geospatial data from the IGN API and other sources.

## 📋 Table of contents

- [Geometric data examples](#geometric-data-examples)
- [Advanced workflow examples](#advanced-workflow-examples)
- [Prerequisites](#prerequisites)
- [Installation](#installation)
- [General notes](#general-notes)

---

## Geometric data examples

### 1. `building_basic.py`

Basic example showing how to create a `Building` (BuildingCollection) and access `GeoCore` properties.

**Run:**

```bash
python examples/building_basic.py
```

**What this example does:**

- Creates a `Building` (BuildingCollection)
- Accesses `GeoCore` properties
- Creates and sets a `PyBoundingBox`
- Displays the properties

---

### 2. `building_from_ign.py`

Complete example showing how to load buildings from the IGN API, process them, and convert to a pandas DataFrame.

**Run:**

```bash
python examples/building_from_ign.py
```

**What this example does:**

- Creates a `BuildingCollection`
- Sets a bounding box (geographic area)
- Downloads buildings from the IGN API via WFS
- Processes heights
- Converts to pandas DataFrame
- Displays statistics

---

### 3. `dem_from_ign.py`

Example showing how to download a Digital Elevation Model (DEM) from the IGN API.

**Run:**

```bash
python examples/dem_from_ign.py
```

**What this example does:**

- Creates a `Dem` instance
- Sets a bounding box
- Downloads the DEM from the IGN API via WMS-R
- Reprojects and saves the GeoTIFF file
- Generates a mask for clipping

---

### 4. `cadastre_from_ign.py`

Example showing how to download cadastral data (parcels) from the IGN API.

**Run:**

```bash
python examples/cadastre_from_ign.py
```

**What this example does:**

- Creates a `Cadastre` instance
- Sets a bounding box
- Downloads cadastral parcels from the IGN API via WFS
- Parses the received GeoJSON
- Saves to GeoJSON

---

### 5. `iris_from_ign.py`

Example showing how to download IRIS statistical units from the IGN API.

**Run:**

```bash
python examples/iris_from_ign.py
```

**What this example does:**

- Creates an `Iris` instance
- Sets a bounding box
- Downloads IRIS units from the IGN API via WFS
- Parses the received GeoJSON
- Saves to GeoJSON

---

### 6. `cosia_from_ign.py`

Complete example showing how to download, vectorize, and convert COSIA (land cover) data from the IGN API to UMEP format.

**Run:**

```bash
python examples/cosia_from_ign.py
```

**What this example does:**

- Downloads the COSIA raster from the IGN API
- Vectorizes the raster by RGB color matching
- Classifies polygons into COSIA classes
- Converts to UMEP classification format
- Rasterizes to UMEP-compatible GeoTIFF

**Additional prerequisites:**

```bash
uv pip install geopandas rasterio numpy shapely
```

---

### 7. `lidar_hd_elevation_from_ign.py`

Example showing how to download LiDAR HD elevation rasters (MNT, MNS, MNH) from the IGN WMS-R service.

**Run:**

```bash
python examples/lidar_hd_elevation_from_ign.py
```

**What this example does:**

- Creates `Mnt`, `Mns`, and `Mnh` instances
- Sets a bounding box in a LiDAR HD covered area
- Downloads terrain, surface, and height models from IGN WMS-R
- Saves GeoTIFF files to `./output/lidar_hd_elevation/`

**Note:** LiDAR HD coverage is limited to acquired zones in France.

---

### 8. `lidar_from_wfs.py`

Example showing how to download and process LiDAR data from the IGN WFS service.

**Run:**

```bash
python examples/lidar_from_wfs.py
```

**What this example does:**

- Creates a `Lidar` instance
- Sets a bounding box
- Downloads LAZ files from the IGN WFS service
- Processes points to create DSM, DTM and CHM rasters
- Saves results as a multi-band GeoTIFF file

**Features:**

- CDSM (Canopy Digital Surface Model) generation from vegetation and water classes
- DSM (Digital Surface Model) generation from ground and building classes
- Filtering by LiDAR classification classes

---

### 8. `rnb_from_api.py`

Example showing how to download RNB (French National Building Reference) data from the RNB API.

**Run:**

```bash
python examples/rnb_from_api.py
```

**What this example does:**

- Creates an `Rnb` instance
- Sets a bounding box
- Downloads building data from the RNB API
- Retrieves GeoJSON data
- Saves to GPKG file

---

### 9. `road_from_ign.py`

Example showing how to download road data from the IGN API.

**Run:**

```bash
python examples/road_from_ign.py
```

**What this example does:**

- Creates a `Road` instance
- Sets a bounding box
- Downloads road data from the IGN API
- Retrieves GeoJSON data
- Saves to GeoJSON

---

### 10. `vegetation_from_ign.py`

Example showing how to compute vegetation from IGN infrared imagery using the NDVI index.

**Run:**

```bash
python examples/vegetation_from_ign.py
```

**What this example does:**

- Creates a `Vegetation` instance
- Sets a bounding box
- Downloads infrared imagery from the IGN API
- Computes the NDVI (Normalized Difference Vegetation Index)
- Filters and polygonizes vegetation
- Retrieves GeoJSON data
- Saves to GeoJSON

**Features:**

- NDVI = (NIR - Red) / (NIR + Red)
- Filtering of pixels with NDVI < 0.2
- Filtering of polygons by minimum area

---

### 11. `water_from_ign.py`

Example showing how to download water body data from the IGN API.

**Run:**

```bash
python examples/water_from_ign.py
```

**What this example does:**

- Creates a `Water` instance
- Sets a bounding box
- Downloads water bodies from the IGN API
- Retrieves GeoJSON data
- Saves to GeoJSON

---

### 12. `lcz_from_url.py`

Example showing how to load LCZ (Local Climate Zone) data from a URL.

**Run:**

```bash
python examples/lcz_from_url.py
```

**What this example does:**

- Creates an `Lcz` instance
- Sets a bounding box
- Loads LCZ data from a zip URL
- Filters by bounding box (spatial overlay)
- Displays the LCZ color table
- Saves to GeoJSON

**Note:** Full LCZ implementation requires reading shapefiles from zip URLs and spatial overlay operations, which are under development.

---

### 14. `umep_workflow_new.py`

Alternative UMEP workflow using the `solweig` Python package (SOLWEIG from UMEP-dev/solweig). Collects DEM and LiDAR (DSM/CDSM), clips rasters, runs SOLWEIG for Tmrt/shadow, post-processes UTCI, and can build animated GIFs from preview PNGs.

**Run:**

```bash
python examples/umep_workflow_new.py
```

**Additional prerequisites:** `uv pip install geopandas rasterio pyproj pillow` and `solweig` (e.g. from UMEP-dev/solweig).

---

### 15. `wind_field_from_ign.py`

Urban wind field (Röckle model) without QGIS: computes `wind_speed.tif` and `wind_direction.tif` from DEM, DSM, and buildings for use in UTCI/SOLWEIG pipelines.

**Run:**

```bash
python examples/wind_field_from_ign.py
```

**What this example does:**

- Optionally downloads DEM and/or generates DSM from LiDAR if not present in `./output`
- Loads buildings from the IGN API
- Runs the Röckle wind solver (`pymdurs.thermal.WindField`) in parallel
- Writes `wind_speed.tif` and `wind_direction.tif` to the output folder

**Output:** `./output/wind_speed.tif`, `./output/wind_direction.tif` (same grid as DEM/DSM).

---

### 16. `utci_rockle_epw.py`

UTCI with Röckle wind field and pythermalcomfort: computes wind speed per pixel for 10 directions (0°, 36°, …, 324°) via the Röckle model, loads weather from an EPW file, runs SOLWEIG for Tmrt, then computes UTCI with `pythermalcomfort` using per-pixel wind speed and writes GeoTIFFs.

**Run:**

```bash
python examples/utci_rockle_epw.py
```

**What this example does:**

- Reuses DEM/DSM (and optionally CDSM/landcover) from `output/umep_workflow` or `output/`
- Loads buildings from the IGN API
- Runs the Röckle wind solver for 10 wind directions and saves `wind_speed_000.tif` … `wind_speed_324.tif` under `output/utci_rockle/wind_10dir/`
- Loads weather (Ta, RH, wind speed, wind direction) from an EPW file (e.g. `la_rochelle_2025.epw`)
- Runs SOLWEIG to get Tmrt per timestep
- For each timestep: selects the nearest Röckle wind raster by EPW wind direction, scales by EPW wind speed, computes UTCI with `pythermalcomfort.models.utci(tdb, tr, v, rh)`, writes a GeoTIFF
- Writes a mean UTCI GeoTIFF over the simulated period

**Output:** `./output/utci_rockle/utci/utci_YYYYMMDD_HHMM.tif` (per timestep), `./output/utci_rockle/utci_mean.tif`.

**Additional prerequisites:** `solweig`, `pythermalcomfort`, `rasterio`. Run `umep_workflow_new.py` or `wind_field_from_ign.py` first to have DEM/DSM (and optionally buildings); place `la_rochelle_2025.epw` in `examples/` or project root.

---

## Apptainer / HPC

Pour exécuter `building_from_ign.py` sur un cluster Linux sans installation locale, utilisez le conteneur Apptainer :

```bash
apptainer build pymdurs.sif apptainer/pymdurs.def
mkdir -p output
apptainer run --bind "$PWD/output:/app/output" pymdurs.sif
```

Voir [apptainer/README.md](../apptainer/README.md) pour la documentation complète (construction, bind mounts, personnalisation).

---

## Prerequisites

### Rust installation

Before installing `pymdurs`, you need to install Rust:

**Windows:**

```bash
# Download and run rustup-init.exe from https://rustup.rs/
# Or use PowerShell:
Invoke-WebRequest -Uri https://win.rustup.rs/x86_64 -OutFile rustup-init.exe
.\rustup-init.exe
```

**macOS:**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**Linux:**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

After installation, restart your terminal or run:

```bash
source $HOME/.cargo/env
```

### pymdurs installation

1. **Clone the repository:**

```bash
git clone https://github.com/rupeelab17/pymdurs.git
cd pymdurs
```

2. **Install pymdurs:**

```bash
# For your architecture (recommended)
maturin develop

# For Apple Silicon specifically
maturin develop --target aarch64-apple-darwin

# For x86_64 on Mac (if needed)
maturin develop --target x86_64-apple-darwin
```

### Python dependencies

**Base dependencies:**

```bash
uv pip install pandas 'numpy<2.0.0'
```

**Important note:** NumPy 2.x may cause compatibility issues with some dependencies (e.g. `numexpr`). Using NumPy < 2.0.0 is recommended. If you already have NumPy 2.x installed, you can downgrade with:

```bash
uv pip install 'numpy<2.0.0' --force-reinstall
```

**Dependencies for advanced workflows:**

```bash
# For geospatial examples
uv pip install geopandas rasterio pyproj shapely

# For umep_workflow.py
uv pip install "solweig @ git+https://github.com/UMEP-dev/solweig.git"
uv pip install umep  # Optional
```

### Internet connection

Examples that use the IGN API require an active internet connection.

---

## General notes

### Default configuration

- **Study area:** Most examples use a bounding box for the La Rochelle area, France
- **Default CRS:** EPSG:2154 (Lambert 93) for French data
- **Input format:** Coordinates must be in WGS84 (EPSG:4326) for the IGN API
- **Output files:** Saved to `./output/` by default

### Limitations

- **Rate limiting:** The IGN API may apply rate limits
- **Data size:** Large areas may take time to download and process
- **Data availability:** Some data may not be available for all areas

### Customization

You can modify the examples to:

- Change the bounding box (your area of interest)
- Change the output CRS
- Adjust processing parameters (default floor height, minimum area, etc.)
- Change the output path

### Output file structure

Generated files are organized as follows:

```
output/
├── building_basic/
├── building_from_ign/
├── dem_from_ign/
├── cadastre_from_ign/
├── iris_from_ign/
├── cosia_from_ign/
├── lidar_from_wfs/
├── rnb_from_api/
├── road_from_ign/
├── vegetation_from_ign/
├── water_from_ign/
├── lcz_from_url/
└── umep_workflow/
    ├── DEM.tif
    ├── DSM.tif
    ├── CDSM.tif
    ├── SVF.tif
    └── ...
```

---

## Support

For more information, see:

- [Main documentation](../README.md)
- [GitHub repository](https://github.com/rupeelab17/pymdurs)
- [IGN documentation](https://geoservices.ign.fr/documentation/services)
