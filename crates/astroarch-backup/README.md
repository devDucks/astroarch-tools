# astroarch-backup

Assistant de sauvegarde AstroArch. Les sauvegardes sont stockées dans un
**fichier image btrfs préalloué** (`astroarch-backup.btrfs`) pour les protéger
de la corruption et des erreurs de manipulation.

**100 % Rust** (interface en QML) : plus aucun fichier C++. Compilation avec Cargo seul.

## Compiler et installer

    cargo build --release
    sudo ./install.sh              # PREFIX=/usr par défaut

Prérequis : Qt 6 (Quick Controls 2), `qt6-tools` (pour `lrelease`, qui compile les
traductions ; sans lui l'interface reste en anglais), `rsync`, `btrfs-progs`, `polkit`.
Sans installation, l'appli retrouve `scripts/astroarch-backup-helper.sh` à côté de
`target/` (variable `ASTROARCH_BACKUP_HELPER` pour forcer un chemin).

## Déroulement de l'assistant

1. **Dossiers à exclure** — une fenêtre externe affiche l'arborescence de
   votre dossier personnel et, s'il existe, du dossier de données partagé
   `/srv/astro-data` (sous-dossiers compris, avec leur taille, dossiers cachés en
   option). Case cochée = dossier exclu avec tout son contenu. Les derniers dossiers exclus sont
   mémorisés et proposés à nouveau. Quelques dossiers indispensables ne peuvent pas être
   exclus (🔒) : `Desktop`, `.astroarch`, `.config`, `.local` et `.oh-my-zsh` du dossier
   personnel, avec leur contenu (le helper les refuse aussi). Si `/srv/astro-data`
   est un autre système de fichiers monté, rsync (`-x`) ne le sauvegarde de toute
   façon pas.
2. **Analyse** — mesure de ce qui sera sauvegardé (tout le système `/` moins les
   exclusions), puis test de l'espace disponible dans `/var/backups`.
3. **Emplacement**
   - Disque principal : **`/var/backups`, imposé** (FHS, non modifiable). Si la
     place suffit, l'utilisateur choisit entre « Enregistrer sur le disque
     principal » et « Utiliser un disque externe ».
   - Si la place ne suffit pas : disque externe **obligatoire**.
4. **Disque externe** — liste (mise à jour automatiquement) de tous les
   systèmes de fichiers qui ne sont pas sur le disque du système, avec bouton
   « Monter » (udisks) pour ceux qui ne sont pas montés. Pour un disque utilisable
   avec assez de place, l'appli demande confirmation de la création de
   `<disque>/astroarch/backups`. Pas assez de place : un autre disque est demandé.
   FAT32 est refusé (limite de 4 Go par fichier).
5. **Prêt** — récapitulatif et **nombre de sauvegardes à conserver** (1 à 50,
   mémorisé), puis lancement.

**Quitter** est possible à tout moment (bouton, croix de la fenêtre). Pendant une
sauvegarde, quitter l'annule : le helper termine son nettoyage de lui-même et
les sauvegardes précédentes ne sont jamais touchées.

## Restauration complète du système

Depuis la première page : « Restaurer une sauvegarde... » → emplacement des sauvegardes
(disque principal ou disque externe) → choix du snapshot → avertissement + confirmation
(saisir `RESTORE`) → restauration **sur le système en marche**, sans autre système.

Principe (« takeover » léger, helper `restore`) :

1. **Vérifications sans rien modifier** : image, snapshot (il doit contenir `/boot`),
   **PARTUUID/UUID** (le `root=` du `cmdline.txt` du snapshot doit être celui de la racine
   actuelle, sinon le Raspberry Pi ne démarrerait plus : refus) et disques cités par le
   `fstab` du snapshot (présents, sauf `nofail`/`noauto`).
2. **Sauvegarde de sécurité** de l'état actuel (case cochée par défaut) : un snapshot de
   plus, sans rotation, pour pouvoir annuler la restauration en la restaurant à son tour.
3. Les outils (bash, rsync, mount, losetup...) sont copiés **avec leurs bibliothèques**
   dans la RAM (`/run/astroarch-restore/root`, un chroot) : ils ne risquent pas d'être
   cassés quand les fichiers du disque changent. Un test les lance avant tout changement.
4. Un service systemd temporaire arrête le bureau et presque tous les services
   (`systemctl isolate`), puis exécute dans ce chroot : `rsync --delete-after` du snapshot
   vers la racine, puis du `boot/` du snapshot vers `/boot`.
5. Redémarrage immédiat (sysrq). La progression s'affiche en mode texte (tty1).

Conséquences, annoncées à l'utilisateur :
- Tout ce qui a changé depuis le snapshot est perdu (fichiers, paquets, mises à jour,
  réglages) : c'est voulu, pour annuler une mise à jour ratée.
- Les dossiers exclus des sauvegardes sont **vidés** (ils existent, vides, dans le snapshot).
  Les disques montés à part (ex. `/srv/astro-data`), `/var/backups` et le contenu de
  `/dev`, `/proc`, `/sys`, `/run`, `/tmp`, `/mnt`, `/media` ne sont jamais touchés.
- Une coupure de courant ou un arrêt manuel pendant la restauration peut rendre le
  système inutilisable : risque assumé par l'utilisateur (avertissement avant de commencer).
- Seuls les snapshots faits **avec** la copie de `/boot` peuvent être restaurés.
- Le journal est copié dans `/var/backups/astroarch-restore-<date>.log` quand les
  sauvegardes sont sur le disque principal.

## Comment ça protège

- **Image btrfs** : checksums sur toutes les données, copy-on-write.
- **Snapshots en lecture seule** (`snapshots/AAAA-MM-JJ_HHMMSS`) créés seulement
  après un rsync réussi : une sauvegarde interrompue, échouée ou annulée ne
  touche jamais aux précédentes. Seuls les N derniers sont gardés (N réglable).
- **Vérification** (`btrfs scrub`) après chaque sauvegarde ; les anciens
  snapshots ne sont supprimés que si elle réussit.
- **Hors sauvegarde** l'image est démontée et immuable (`chattr +i`) : même root
  doit d'abord retirer l'attribut.
- **Partition de démarrage** : rsync tourne avec `-x` (une seule partition), donc
  `/boot` (FAT32) n'est pas traversé. Les partitions de démarrage montées à part
  (`/boot`, `/boot/efi`, `/boot/firmware`) sont copiées dans une seconde passe,
  dans `work/boot` (etc.), donc présentes dans chaque snapshot ; elles comptent
  aussi dans la mesure d'espace. Un `/boot` qui fait partie de `/` est déjà inclus.
  Comme la FAT n'a ni ACL ni attributs étendus, ils ne sont pas copiés pour elle.
- rsync copie avec `--inplace --no-whole-file` : un fichier modifié ne réécrit
  que ses blocs changés (sinon chaque snapshot garderait une copie complète).
- Limite : en profil de données `single`, btrfs *détecte* la corruption mais ne
  la *répare* pas (voir `DATA_PROFILE` dans le helper : `dup` répare, mais
  double la place nécessaire). Une image sur le disque principal ne protège pas
  d'une panne de ce disque.
- Prérequis : `btrfs-progs` et le module noyau `btrfs`.

## Taille du fichier image

- À la création : données (dossiers exclus déduits) + 20 % + 2 Gio (minimum
  4 Gio), sans compter la compression zstd.
- Ensuite la taille suit l'usage RÉEL : avant chaque copie, le helper mesure ce
  qu'il reste à ajouter (rsync à blanc) et n'agrandit l'image que si la place
  libre interne (compression comprise) ne suffit pas.
- **Réduction automatique** : après une sauvegarde réussie et vérifiée, si le
  fichier a au moins 1 Gio ET 15 % de marge en trop, il est réduit à
  « utilisé + 15 % + 1 Gio », puis revérifié. Un échec de la réduction ne fait
  jamais échouer la sauvegarde.
- Les anciens snapshots gardent les anciennes versions des fichiers modifiés :
  l'espace utilisé monte jusqu'à N snapshots puis se stabilise.

## Structure

```
Cargo.toml           dépendances Rust (cxx-qt 0.10, serde, serde_json, thiserror)
build.rs             bridge cxx-qt + module QML + compilation des traductions (lrelease)
install.sh           installation (binaire, helper, polkit, lanceur)
org.astroarch.backup.desktop   lanceur (icône, nom de l'application)
src/
  main.rs            démarrage : QGuiApplication + chargement de Main.qml
  bridge.rs          QObject BackupManager exposé à QML (lance le helper)
  storage.rs         contrôles de l'emplacement, évaluation de l'espace, helper
  media.rs           détection des disques externes (lsblk) et montage (udisks)
  disk_analysis.rs   mesure de la taille à sauvegarder (via le helper)
  config.rs          configuration (dernier emplacement, rétention)
scripts/
  astroarch-backup-helper.sh  SEUL code exécuté en root (pkexec)
  org.astroarch.backup.policy action polkit (mot de passe gardé quelques minutes)
qml/
  Main.qml               assistant (5 écrans) et dialogues
  FolderTreeWindow.qml   fenêtre externe : arborescence avec cases à cocher
translations/
  qml_fr.ts, qml_it.ts, qml_es.ts, qml_de.ts
```

## Helper root

`astroarch-backup-helper backup|measure|shrink|explain|mount|umount|usage|prune|snapshots|restore ...`
valide tous ses arguments (il construit lui-même la commande rsync : l'appli ne
lui passe jamais d'options rsync). Les dossiers exclus sont des chemins absolus
situés dans le dossier personnel de l'appelant ou dans `/srv/astro-data`, sans `..`
ni motif rsync.
`/var/backups` n'est jamais sauvegardé (une image peut y vivre). L'annulation
passe par un fichier-drapeau (le helper tourne en root, l'appli ne peut pas le
tuer). Protocole de sortie et codes d'erreur : voir l'en-tête du script.

Le helper est installé à un chemin fixe (`/usr/lib/astroarch-backup/`) : le
fichier polkit y fait référence.

## Voir ce qui est sauvegardé

L'image n'est pas visible tant qu'elle est démontée. Pour regarder dedans (le
dossier est `/var/backups` ou `<disque>/astroarch/backups`) :

    H=/usr/lib/astroarch-backup/astroarch-backup-helper   # ou scripts/astroarch-backup-helper.sh
    pkexec $H explain --dest /var/backups [--exclude CHEMIN]...   # ce que la mesure compte, par taille
    pkexec $H mount   --dest /var/backups      # montage LECTURE SEULE sur /run/astroarch-backup/browse
    ls /run/astroarch-backup/browse/snapshots  # un dossier par sauvegarde
    pkexec $H umount  --dest /var/backups
    pkexec $H usage   --dest /var/backups      # place utilisée + part de chaque snapshot
    pkexec $H prune   --dest /var/backups --keep 2   # ne garder que 2 snapshots (IRRÉVERSIBLE), puis réduit le fichier
    pkexec $H shrink  --dest /var/backups      # réduire à la demande

Pendant qu'elle est montée pour la consulter, une sauvegarde ou une réduction
est refusée (il faut d'abord `umount`).

## Traductions

Les textes sont écrits en anglais dans les .qml avec `qsTr("...")`. Aucun code
n'est nécessaire pour les charger : `QQmlApplicationEngine` charge tout seul
`qml_<langue>.qm` depuis le dossier `i18n` voisin de `Main.qml`. `build.rs` compile
`translations/qml_<langue>.ts` avec `lrelease` et les place à cet endroit. La langue
suit celle du système (LANG), repli sur l'anglais.
Mise à jour : `lupdate qml/*.qml -ts translations/qml_fr.ts` (idem pour it, es, de).

## Pas encore fait

- Restauration depuis un snapshot (monter l'image en lecture seule puis copier).
- Ce mécanisme n'a pas pu être testé sur un Raspberry Pi : essayez-le d'abord sur une
  copie ou un disque de rechange.
