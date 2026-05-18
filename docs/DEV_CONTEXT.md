# Contexte développeur

## Objectif du projet
Application GTK4 en Rust pour effacer des disques avec `shred`.
Elle liste les disques détectés, permet une sélection multiple et lance les effacements en parallèle.

## Architecture rapide
- src/main.rs : point d’entrée, vérifie la présence de pkexec.
- src/ui.rs : interface GTK (liste disques, boutons, progression, select all).
- src/disk.rs : détection des disques via `lsblk` + vérifications système.
- src/shred.rs : lancement/arrêt de `shred` via pkexec, parsing de progression.
- src/system.rs : smartctl et blkid via le helper privilégié.
- src/paths.rs : chemin du helper (`SHRED_GRAPHIQUE_HELPER` ou `/usr/bin/...`).
- src/logger.rs : journalisation des opérations.

## Points clés
- Elevation GUI : pkexec est requis, le prompt apparait au lancement d'une suppression.
- Helper : binaire `shred-graphique-helper` + policy polkit.
- Progression : `shred` émet des étapes, combinées en progression globale.
- Arrêt : bouton Stop appelle un kill sur le PID.
- Rafraîchissement : bouton dédié, désactivé pendant les opérations.

## Détection disques
- `lsblk -d -n -P -o NAME,SIZE,MODEL,TYPE`
- Timeout pour éviter les blocages lors d’un débranchement.
- Filtre les loop/ram et non-disk.

## Dépendances système (Ubuntu/Debian)
Gérées par `make deps` via `scripts/deps.sh` :
- build-essential, pkg-config, libgtk-4-dev, libglib2.0-dev
- policykit-1, coreutils, util-linux, smartmontools
- Rust (rustup) si absent

## Workflow de développement

```bash
make deps      # installe et vérifie les dépendances (échoue si incomplet)
make dev       # cargo build --release + install sous PREFIX (/usr/local)
make run       # lance l'app avec SHRED_GRAPHIQUE_HELPER pointant vers PREFIX
make test      # cargo test
make clean     # cargo clean
make deb       # paquet .deb (production, helper sous /usr/bin)
```

Variable optionnelle : `PREFIX=/usr/local` (défaut).

## Profil release
- Optimisations taille : opt-level=z, lto, codegen-units=1, panic=abort, strip=symbols

## Packaging
- Script .deb : packaging/build_deb.sh
- Desktop entry : packaging/shred-graphique.desktop
- Dépendances runtime .deb : libgtk-4-1, policykit-1, coreutils, smartmontools, util-linux

## Tests manuels essentiels
- Branche/retire un disque et clique sur Rafraîchir.
- Lance une suppression et vérifie la progression.
- Débranche un disque en cours et vérifie l’erreur.
- Stop pendant une opération et vérifie l’état.
