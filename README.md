# Shred Graphique

Application **Rust** (GTK4) pour l’effacement sécurisé de disques sous Linux, via la commande [`shred`](https://www.gnu.org/software/coreutils/manual/html_node/shred-invocation.html). Pensée pour un usage en **atelier / banc de nettoyage** : détection des disques branchés, sélection multiple, effacements en parallèle avec suivi de progression.

> **Attention** — Cette application détruit définitivement les données des disques sélectionnés. Vérifiez toujours les cibles avant de lancer une suppression. Le disque système est protégé, mais une erreur d’identification reste possible sur d’autres supports.

## Fonctionnalités

- Liste des disques physiques détectés (`lsblk`), avec rafraîchissement à chaud
- Sélection multiple (jusqu’à **10 disques** en parallèle)
- Effacement via `shred` (3 passes + zéros finaux) avec barre de progression par disque
- Arrêt d’une opération en cours (bouton **Stop**)
- Élévation des privilèges via **PolicyKit** (`pkexec`) et helper dédié
- Consultation **SMART** (`smartctl`) et vérification post-effacement (`blkid`)
- Journalisation des opérations (fichiers JSON dans le répertoire utilisateur)
- Paquet **`.deb`** pour installation système

## Prérequis

| Composant | Rôle |
|-----------|------|
| Linux (Debian/Ubuntu pour `make deps`) | Plateforme cible |
| GTK 4 | Interface graphique |
| `shred`, `coreutils` | Effacement |
| `policykit-1` / `pkexec` | Droits administrateur |
| `util-linux` | `lsblk`, `findmnt`, `blkid` |
| `smartmontools` | `smartctl` |
| Rust (stable) | Compilation |

## Installation rapide

### Depuis les sources (développement)

```bash
git clone <url-du-repo>
cd shred_graphique

make deps    # installe et vérifie les dépendances (apt)
make dev     # compile en release + installe sous /usr/local (sudo)
make run     # lance l'application
```

Après une installation fraîche de Rust :

```bash
source "$HOME/.cargo/env"
```

Installation dans un autre préfixe :

```bash
make dev PREFIX=$HOME/.local
make run PREFIX=$HOME/.local
```

### Paquet Debian

```bash
make deb
# Artefact : dist/shred-graphique_<version>_<arch>.deb
sudo dpkg -i dist/shred-graphique_*.deb
```

Dépendances runtime du paquet : `libgtk-4-1`, `policykit-1`, `coreutils`, `smartmontools`, `util-linux`.

## Utilisation

1. Lancer **Shred Graphique** (menu applications ou `make run` / `shred-graphique`).
2. Cliquer sur **Rafraîchir** si un disque vient d’être branché.
3. Sélectionner les disques à effacer (case à cocher ou **Tout sélectionner**).
4. Vérifier que les cibles ne sont pas le disque système ni un support critique.
5. Lancer **la suppression sécurisée** et confirmer.
6. Saisir le mot de passe administrateur lorsque PolicyKit le demande.

Pendant l’effacement : progression par disque, bouton **Stop** pour interrompre une opération, autres disques grisés jusqu’à la fin.

Guide détaillé : [docs/USER_GUIDE.md](docs/USER_GUIDE.md).

## Commandes Make

| Commande | Description |
|----------|-------------|
| `make help` | Aide des cibles |
| `make deps` | Dépendances système + Rust (vérifie, installe si besoin) |
| `make dev` | Build release + installation locale (binaires + polkit) |
| `make run` | Lance l’app installée sous `PREFIX` |
| `make release` | Compile en release sans installation |
| `make test` | Tests unitaires Rust |
| `make deb` | Construit le paquet `.deb` |
| `make clean` | `cargo clean` |

## Architecture (aperçu)

```
src/
  main.rs              # Point d'entrée, vérifications outils
  ui.rs                # Interface GTK4
  disk.rs              # Détection disques (lsblk)
  shred.rs             # Exécution / arrêt shred via pkexec
  system.rs            # SMART et blkid
  paths.rs             # Chemin du helper (env SHRED_GRAPHIQUE_HELPER)
  bin/
    shred-graphique-helper.rs   # Helper privilégié (shred, kill, smartctl, blkid)

scripts/
  deps.sh              # Installation des dépendances (apt)
  install-dev.sh       # Installation locale pour le dev

packaging/
  build_deb.sh         # Construction du .deb
  shred-graphique.policy   # Règle PolicyKit
```

En production, le helper est installé sous `/usr/bin/shred-graphique-helper`. En développement, `make dev` l’installe sous `PREFIX` (défaut `/usr/local`) et `make run` définit `SHRED_GRAPHIQUE_HELPER` en conséquence.

Documentation développeur : [docs/DEV_CONTEXT.md](docs/DEV_CONTEXT.md).

## Sécurité

- Les opérations sur disques passent par **pkexec** et un binaire helper déclaré dans une policy Polkit.
- Le disque hébergeant la racine du système est **refusé** pour l’effacement.
- Les effacements sont **irréversibles** ; seule l’interruption immédiate du processus `shred` peut limiter les dégâts sur une opération en cours.

## Licence

[MIT](LICENSE) — Copyright (c) 2026 Shred Graphique Contributors
