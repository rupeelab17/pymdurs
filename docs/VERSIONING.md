# Gestion des versions

La version est définie à un seul endroit logique et synchronisée dans :

- `pyproject.toml`
- `pymdurs/Cargo.toml`
- `rsmdu/Cargo.toml`

## Scripts (depuis la racine du dépôt)

### Définir une version exacte

```bash
./scripts/set-version.sh 0.1.2
```

Met à jour les trois fichiers avec la version indiquée (y compris avec suffixe, ex. `0.1.2.dev0`).

### Incrémenter la version (patch / minor / major)

```bash
./scripts/bump-version.sh patch   # 0.1.1 → 0.1.2
./scripts/bump-version.sh minor  # 0.1.1 → 0.2.0
./scripts/bump-version.sh major   # 0.1.1 → 1.0.0
```

Modifie uniquement les fichiers ; aucun commit ni tag.

### Release : bump + commit + tag

Pour publier une nouvelle version :

```bash
./scripts/bump-version.sh patch --tag
```

Cela :

1. Incrémente la version (patch par défaut)
2. Met à jour les trois fichiers
3. Fait un commit `chore: release X.Y.Z`
4. Crée le tag `py-X.Y.Z`

Puis pousser :

```bash
git push && git push origin py-0.1.2
```

Ensuite, lancer le workflow **Release Python** sur GitHub (Actions) pour construire et publier sur PyPI.

### Version de développement (après une release)

Pour repasser en mode « en cours de dev » après une release :

```bash
./scripts/bump-version.sh patch --dev   # 0.1.2 → 0.1.3.dev0
```

Puis commit comme d’habitude. Au prochain release, utiliser `bump-version.sh patch --tag` (le `.dev0` est ignoré pour le calcul du prochain numéro).

## Résumé du flux

| Action | Commande |
|--------|----------|
| Changer la version à la main | `./scripts/set-version.sh 0.1.2` |
| Préparer une release (bump + commit + tag) | `./scripts/bump-version.sh patch --tag` |
| Pousser le tag | `git push && git push origin py-0.1.2` |
| Publier sur PyPI | Lancer le workflow « Release Python » dans GitHub Actions |
