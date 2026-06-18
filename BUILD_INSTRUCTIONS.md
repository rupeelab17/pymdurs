# Instructions de build pour pymdurs

## Structure du projet

```
pymdurs/                    # Racine du projet (pyproject.toml)
├── pymdurs/                # Package Python + extension Rust (Cargo.toml)
│   ├── src/                # Code Rust (bindings PyO3)
│   │   └── bindings/
│   └── tests/
├── rsmdu/                  # Bibliothèque Rust (dépendance du package pymdurs)
├── rsmdu-wasm/            # Build WebAssembly (séparé)
└── examples/              # Exemples Python
```

**Important** : Les commandes maturin doivent être exécutées depuis la **racine du projet** (où se trouve `pyproject.toml`). Maturin utilise `manifest-path = "pymdurs/Cargo.toml"`.

---

## Installer uv

Le projet utilise [uv](https://docs.astral.sh/uv/) pour la gestion des dépendances (`uv.lock`). Installer uv avant de builder :

**macOS / Linux** ([installateur Astral](https://docs.astral.sh/uv/getting-started/installation/)) :

```bash
curl -LsSf https://astral.sh/uv/install.sh | sh
```

**Windows (PowerShell) :**

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://astral.sh/uv/install.ps1 | iex"
```

**Alternatives :** `brew install uv`, `pipx install uv`, ou `pip install uv`.

Créer l’environnement virtuel :

```bash
uv venv .venv --python 3.13
```

Synchroniser l’environnement du projet après clonage :

```bash
uv sync
```

Mise à jour de uv (installateur standalone) : `uv self update`.

---

## macOS

### Prérequis

```bash
brew install gdal
```

### Cibles Rust

| Architecture               | Target                 |
| -------------------------- | ---------------------- |
| Apple Silicon (M1, M2, M3) | `aarch64-apple-darwin` |
| Intel (x86_64)             | `x86_64-apple-darwin`  |

### Commandes

```bash
# Apple Silicon
maturin develop --target aarch64-apple-darwin
# ou avec uv
uv run maturin develop --target aarch64-apple-darwin

# Intel
maturin develop --target x86_64-apple-darwin
```

### Build release / wheel

```bash
maturin build --target aarch64-apple-darwin --release   # Apple Silicon
maturin build --target x86_64-apple-darwin --release    # Intel
```

### Erreur maturin : `Cannot repair wheel` et `libabsl_…` (macOS)

Après la compilation Rust, maturin analyse les dépendances dynamiques (GDAL → abseil/protobuf, etc.) pour les copier dans la roue. Les install names du type `@rpath/libabsl_debugging_internal….dylib` sont résolus comme le ferait dyld, avec des répertoires de secours **limités** par défaut (`~/lib`, `/usr/local/lib`, `/lib`, `/usr/lib`). Sur **Apple Silicon**, Homebrew installe les bibliothèques dans `$(brew --prefix)/lib` (souvent `/opt/homebrew/lib`), qui n’est pas dans cette liste : la résolution échoue et le repair s’arrête.

**Correctif** : inclure le répertoire `lib` de Homebrew dans `DYLD_FALLBACK_LIBRARY_PATH` pendant le build / `pip install` / `uv pip install` :

```bash
export DYLD_FALLBACK_LIBRARY_PATH="$(brew --prefix)/lib${DYLD_FALLBACK_LIBRARY_PATH:+:$DYLD_FALLBACK_LIBRARY_PATH}"
# puis, par exemple :
uv pip install -e .
# ou
maturin build --release
```

En dernier recours (roue non portable, déconseillé pour une publication), on peut désactiver le repair dans `pyproject.toml` : `auditwheel = "skip"` sous `[tool.maturin]`.

---

## Linux

### Prérequis (Debian/Ubuntu)

```bash
sudo apt-get update
sudo apt-get install -y libgdal-dev gdal-bin libclang-dev
```

### Cibles Rust

| Architecture    | Target                      |
| --------------- | --------------------------- |
| x86_64          | `x86_64-unknown-linux-gnu`  |
| ARM64 (aarch64) | `aarch64-unknown-linux-gnu` |

### Commandes

```bash
# x86_64
maturin develop --target x86_64-unknown-linux-gnu
# ou sans target (utilise l'architecture native)
maturin develop

# ARM64
maturin develop --target aarch64-unknown-linux-gnu
```

### Build release / wheel

```bash
maturin build --target x86_64-unknown-linux-gnu --release
maturin build --target aarch64-unknown-linux-gnu --release
```

---

## Windows

### Prérequis

1. **OSGeo4W** : installer GDAL, GEOS, PROJ, SQLite3 (via [OSGeo4W](https://trac.osgeo.org/osgeo4w/) ou l’action `echoix/setup-OSGeo4W` en CI)
2. **Chocolatey** : installer LLVM, pkg-config, SQLite
   ```powershell
   choco install llvm pkgconfiglite sqlite -y
   ```
3. Variables d’environnement (exemple avec OSGeo4W installé dans `C:\OSGeo4W`) :
   - `GDAL_HOME` = racine OSGeo4W
   - `PKG_CONFIG_PATH` = `%GDAL_HOME%\lib\pkgconfig`
   - `PATH` : inclure les binaires Rust, OSGeo4W et Chocolatey

### Cible Rust

| Architecture | Target                   |
| ------------ | ------------------------ |
| x86_64       | `x86_64-pc-windows-msvc` |

### Commandes

```powershell
maturin develop --target x86_64-pc-windows-msvc
```

### Build release / wheel

```powershell
maturin build --target x86_64-pc-windows-msvc --release
```

---

## Vérifier l'installation

```bash
python -c "import pymdurs; print('OK')"
```

## Notes

- Le package **pymdurs** (et non rsmdu) est le module Python exposé
- Le package utilise PyO3 avec `abi3-py38` pour la compatibilité Python 3.8+
- Le projet utilise **uv** pour la gestion des dépendances (`uv.lock`)
- Les warnings de compilation (variables non utilisées, nommage) sont mineurs et n'empêchent pas l'utilisation
