# Guide utilisateur (tutoriel simple)

## Objectif
Cette application permet d’effacer définitivement des disques via `shred`.
Elle est conçue pour un usage interne en atelier / banc de nettoyage.

## Prérequis
- Être sur Linux
- Disposer des droits sudo (un mot de passe sera demandé au lancement)

## Installation des dependances
Avant la premiere compilation, lancer :

```bash
make deps
```

Cette commande verifie puis installe automatiquement les dependances manquantes
sur Debian/Ubuntu (apt).

## Étapes rapides
1. Ouvrir l’application.
2. Cliquer sur **Rafraîchir la liste** si vous branchez un disque à chaud.
3. Sélectionner jusqu’à 10 disques.
4. Vérifier que les disques affichés ne sont pas le disque système.
5. Cliquer sur **Lancer la Suppression Sécurisée**.
6. Confirmer.

## Pendant l’effacement
- Une demande de mot de passe (fenetre systeme) peut apparaitre au lancement.
- La progression s’affiche pour chaque disque.
- Le bouton **Stop** permet d’arrêter un disque en cours.
- Les disques non sélectionnés sont grisés jusqu’à la fin.

## Fin d’opération
- Le statut global indique la fin.
- Vous pouvez relancer une opération ou rafraîchir la liste.

## Problèmes courants
- **Pas de disques visibles** : cliquez sur **Rafraîchir la liste**.
- **Erreur pendant l’effacement** : le disque peut avoir été débranché.
