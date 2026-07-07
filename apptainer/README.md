# Conteneur Apptainer pymdurs (exemple buildings)

Image Apptainer (`.sif`) pour exécuter [`examples/building_from_ign.py`](../examples/building_from_ign.py) sur un cluster HPC Linux, sans installation locale de pymdurs.

**Prérequis :** Linux avec [Apptainer](https://apptainer.org/docs/user/main/quick_start.html) installé. La construction et l'exécution se font sur le cluster (Apptainer n'est pas disponible nativement sur macOS).

## Construction du `.sif`

Depuis la racine du dépôt, sur un nœud de build du cluster :

```bash
cd pymdurs
apptainer build pymdurs.sif apptainer/pymdurs.def
```

Si `fakeroot` est disponible sur le cluster :

```bash
apptainer build --fakeroot pymdurs.sif apptainer/pymdurs.def
```

Le fichier `pymdurs.sif` (~500 Mo+) ne doit pas être versionné dans git.

## Exécution

L'exemple télécharge les bâtiments depuis l'API IGN (WFS BDTOPO). **Un accès réseau est requis au runtime.**

```bash
# Créer le répertoire de sortie et lancer l'exemple par défaut
mkdir -p output
apptainer run --bind "$PWD/output:/app/output" pymdurs.sif
```

Les fichiers produits sont écrits dans `./output/` sur l'hôte :

- `buildings.shp` (+ `.shx`, `.dbf`, `.prj`, …)
- `buildings.gpkg`
- `buildings.geojson`
- fichiers intermédiaires pymdurs sous `output/`

### Autres commandes utiles

```bash
# Shell interactif dans le conteneur
apptainer shell pymdurs.sif

# Exécution explicite du script
apptainer exec --bind "$PWD/output:/app/output" pymdurs.sif \
  python examples/building_from_ign.py

# Vérifier l'installation de pymdurs
apptainer exec pymdurs.sif python -c "import pymdurs; print(pymdurs.__version__)"

# Aide intégrée au conteneur
apptainer run-help pymdurs.sif
```

## Bind mounts

Apptainer monte par défaut `$HOME` et le répertoire courant. Le bind explicite `--bind "$PWD/output:/app/output"` est nécessaire pour écrire les exports dans un répertoire persistant sur l'hôte (le filesystem du `.sif` est en lecture seule).

La variable d'environnement `PYMDURS_OUTPUT=/app/output` est définie dans l'image ; le script `building_from_ign.py` l'utilise pour les sorties.

## Personnalisation

- **Zone d'étude :** modifier la bbox WGS84 dans `examples/building_from_ign.py` avant de reconstruire le `.sif`, ou bind-monter une version personnalisée :

  ```bash
  apptainer run --bind "$PWD/output:/app/output" \
    --bind "$PWD/my_building_from_ign.py:/app/examples/building_from_ign.py" \
    pymdurs.sif
  ```

- **Bbox par défaut :** zone Lagord / La Rochelle, France (`EPSG:4326`).

## Validation

```bash
apptainer build pymdurs.sif apptainer/pymdurs.def
apptainer exec pymdurs.sif python -c "import pymdurs; print(pymdurs.__version__)"
mkdir -p output
apptainer run --bind "$PWD/output:/app/output" pymdurs.sif
ls -la output/buildings.*
```

## Voir aussi

- [Dockerfile](../Dockerfile) — image Docker équivalente
- [examples/README.md](../examples/README.md) — documentation des exemples Python
