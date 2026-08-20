# Tidyrs — logo (option 1a)

Barre d'en-tête + colonnes de hauteurs variées : une table qui tient debout.
Géométrie 100 × 96 (rectangles uniquement) — nette de 16 px à l'affiche.

## Fichiers

- `svg/mark-*.svg` — symbole seul (light / dark / `currentColor`)
- `svg/lockup-*.svg` — symbole + logotype, avec ou sans fond
- `svg/avatar-*.svg` — carré 1:1 (avatar GitHub, crates.io)
- `svg/favicon.svg` + `png/favicon-16|32.png`
- `svg/banner-dark.svg` — bandeau README 1280 × 320
- `png/` — exports 16 → 1024 px
- `ascii.txt` — transposition terminal

## Couleurs

| rôle | oklch | hex |
| --- | --- | --- |
| encre | `oklch(.24 .012 70)` | `#262320` |
| accent rouille | `oklch(.58 .14 40)` | `#bb6240` |
| accent rouille (sur fond sombre) | `oklch(.68 .14 40)` | `#d97e5a` |
| clair | `oklch(.95 .004 80)` | `#f1efec` |
| fond sombre | `oklch(.20 .012 70)` | `#201e1b` |

## Typographie

IBM Plex Sans 600 (logotype, `letter-spacing: -.03em`) · IBM Plex Mono (baseline, `.24em`).
Le texte des SVG n'est pas vectorisé : pour un usage hors-web, convertir en courbes ou installer IBM Plex.

## Règles

- Marge de protection : la largeur d'une colonne (0.16 × hauteur du symbole) sur les 4 côtés.
- L'accent rouille reste sur la barre d'en-tête uniquement — jamais sur les colonnes.
- En dessous de 24 px, préférer `mark-*` seul (sans logotype).
- Version 1 couleur : `mark-mono.svg` hérite de `currentColor`.
