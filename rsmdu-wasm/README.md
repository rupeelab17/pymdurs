# rsmdu-wasm

WebAssembly bindings for building GeoJSON processing in the browser.

## Features

- Load cadastral parcels from IGN WFS API
- Load buildings from GeoJSON
- Process building heights (fill missing heights using defaults or mean)
- Convert building collections to GeoJSON
- Get building statistics (count, total area, mean height, etc.)

## Note

This is a **standalone WASM implementation** that doesn't depend on the full `rsmdu` crate (which includes GDAL, proj, geos, etc. that are not WASM-compatible). It provides the core building processing functionality using only WASM-compatible libraries (geo, geojson).

## Building

### Prerequisites

- Rust (latest stable) - **Recommended: Install via rustup** (not Homebrew)
- `wasm-pack` - Install with: `cargo install wasm-pack`
- `wasm32-unknown-unknown` target - Install with: `rustup target add wasm32-unknown-unknown`

### Troubleshooting

If you get an error about `wasm32-unknown-unknown target not found`:

1. **If using rustup** (recommended):

   ```bash
   rustup target add wasm32-unknown-unknown
   ```

2. **If using Homebrew Rust**:

   - Consider switching to rustup: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
   - Or follow: https://rustwasm.github.io/wasm-pack/book/prerequisites/non-rustup-setups.html

3. **If rustup cache issues**:
   ```bash
   # Create cache directory if missing
   mkdir -p ~/.rustup/tmp
   chmod 755 ~/.rustup/tmp
   rustup target add wasm32-unknown-unknown
   ```

### Build Steps

1. **Fix wasm32-unknown-unknown target** (if needed):

```bash
cd rsmdu-wasm
./fix-wasm-target.sh
```

2. Build the WASM package:

```bash
./build.sh
```

Or manually:

```bash
wasm-pack build --target web --out-dir examples/pkg
```

2. Serve the example:

```bash
cd examples
# Using Python 3
python3 -m http.server 8000

# Or using Node.js
npx serve .

# Or using any other static file server
```

3. Open in browser:

```
http://localhost:8000/index.html      # buildings
http://localhost:8000/dem.html        # DEM raster
http://localhost:8000/cadastre.html   # cadastral parcels (PCI)
```

## Usage

### JavaScript/TypeScript

```javascript
import init, { WasmBuildingCollection } from "./pkg/rsmdu_wasm.js";

// Initialize WASM
await init("./pkg/rsmdu_wasm_bg.wasm");

// Load buildings from GeoJSON
const geojson = {
  type: "FeatureCollection",
  features: [
    {
      type: "Feature",
      geometry: {
        type: "Polygon",
        coordinates: [
          [
            [-1.15, 46.16],
            [-1.14, 46.16],
            [-1.14, 46.17],
            [-1.15, 46.17],
            [-1.15, 46.16],
          ],
        ],
      },
      properties: {
        hauteur: 10.5,
        nombre_d_etages: 3,
      },
    },
  ],
};

const collection = WasmBuildingCollection.from_geojson(
  JSON.stringify(geojson),
  3.0 // default storey height in meters
);

// Process heights
collection.process_heights();

// Get processed GeoJSON
const processedGeojson = collection.to_geojson();
const processedData = JSON.parse(processedGeojson);

// Get statistics
const stats = collection.get_stats();
console.log("Building count:", stats.count);
console.log("Total area:", stats.total_area);
console.log("Mean height:", stats.mean_height);

// Clean up
collection.free();
```

## API Reference

### `WasmBuildingCollection`

#### Constructor

- `new(default_storey_height: number)`: Create a new empty building collection

#### Static Methods

- `from_geojson(geojson_str: string, default_storey_height: number)`: Load buildings from GeoJSON string

#### Instance Methods

- `len()`: Get the number of buildings
- `is_empty()`: Check if the collection is empty
- `process_heights()`: Process building heights (fill missing heights)
- `to_geojson()`: Convert to GeoJSON string
- `get_stats()`: Get building statistics (returns JS object with count, total_area, mean_height, buildings_with_height)
- `free()`: Explicitly free the collection (optional, handled automatically)

## Example

See `examples/index.html`, `examples/dem.html`, and `examples/cadastre.html` for Leaflet demos.

## License

GPL-3.0
