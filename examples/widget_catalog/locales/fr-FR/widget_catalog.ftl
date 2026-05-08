# FernUI Widget Catalog — French translations.

# ── App chrome ──────────────────────────────────────────────────────────
app-title = FernUI — Catalogue de Widgets
app-subtitle = glisser · double-clic pour agrandir · clic droit pour le menu
app-unsupported-chrome = (chrome personnalisé non pris en charge sur cette plateforme — repli sur les décorations natives)

# ── View mode toggle ────────────────────────────────────────────────────
mode-label = DSL fern!
mode-tooltip = Bascule chaque onglet entre la version classique (constructeur) et la version macro fern! du même arbre.

# ── Locale switcher ─────────────────────────────────────────────────────
locale-en = English
locale-fr = Français
locale-ar = العربية

# ── Theme switcher ──────────────────────────────────────────────────────
theme-label = Thème
theme-tooltip = Basculer entre les thèmes clair et sombre.

# ── Tab titles ──────────────────────────────────────────────────────────
tab-palette-title = Palette
tab-layout-title = Disposition
tab-visuals-title = Visuels
tab-containers-title = Conteneurs
tab-chrome-title = Chrome
tab-buttons-title = Boutons
tab-inputs-title = Saisies
tab-indicators-title = Indicateurs
tab-text-title = Texte
tab-datetime-title = Date & heure
tab-color-title = Couleur
tab-menus-title = Menus
tab-overlays-title = Surcouches
tab-data-title = Données
tab-animations-title = Animations
tab-settings-title = Paramètres

# ── Deep-dive references ────────────────────────────────────────────────
tab-palette-refs = Tous les rôles de surface, de texte et d'éditeur, avec un pangramme texte enrichi + emojis pour rendre le changement de thème visible. Voir : docs/reactive-theme.md.
tab-layout-refs = Piles, grilles et distribution du jeu. Voir : cargo run -p text_and_layout, cargo run -p split_view.
tab-visuals-refs = Primitives de rendu, icônes, images, bandes de validation. Voir : cargo run -p simple_button.
tab-containers-refs = Panneaux, cartes, accordéons, zones de défilement, vues fractionnées. Voir : cargo run -p split_view, cargo run -p tool_box.
tab-chrome-refs = Chrome de l'application : barre d'outils, barre d'état, fil d'Ariane, assistants, bannières. Voir : cargo run -p title_bar_demo.
tab-buttons-refs = Toutes les variantes de bouton. Voir : cargo run -p simple_button, cargo run -p menus_and_dropdowns.
tab-inputs-refs = Saisies booléennes et de sélection. La navigation clavier approfondie est dans les exemples dédiés.
tab-indicators-refs = État en lecture seule : progression, sabliers, badges, avatars, liens.
tab-text-refs = Édition de texte simple ligne et riche. Voir : cargo run -p rich_text_editor, cargo run -p spin_box.
tab-datetime-refs = Calendrier, sélecteurs de date, d'heure et de plage. Voir : cargo run -p datetime_pickers.
tab-color-refs = Saisie hexadécimale, édition compacte, sélecteur HSV complet. Voir : cargo run -p color_picker.
tab-menus-refs = Barre de menu, listes, menus contextuels. Voir : cargo run -p menus_and_dropdowns.
tab-overlays-refs = Infobulles, popovers, dialogues, snackbars. Voir : cargo run -p tooltips_showcase, cargo run -p dialogs_and_popovers, cargo run -p file_dialogs.
tab-data-refs = ListView, TreeView, TableView, TreeTable. Voir : cargo run -p data_grid, cargo run -p tree_table, cargo run -p data_collections.
tab-animations-refs = Fade, pulse, slide, blur et leurs amis. Voir : cargo run -p animations, cargo run -p animations_kit.
tab-settings-refs = Widgets de réaffectation des raccourcis et de paramètres de confidentialité. Voir : cargo run -p shortcuts_demo.

# ── Tab body placeholder ────────────────────────────────────────────────
stub-heading = Bientôt disponible
stub-body = Cet onglet sera rempli lors des phases 3/4 de la réécriture du catalogue.

# ── Common reusable demo labels ────────────────────────────────────────
demo-save = Enregistrer
demo-cancel = Annuler
demo-open = Ouvrir
demo-new = Nouveau
demo-edit = Édition
demo-quit = Quitter
demo-undo = Annuler
demo-redo = Rétablir
demo-cut = Couper
demo-copy = Copier
demo-paste = Coller
demo-find = Rechercher…
demo-confirm = Confirmer
demo-learn-more = En savoir plus
demo-next = Suivant
demo-back = Retour
demo-finish = Terminer
demo-loading = Chargement…

# ── Indicators tab ──────────────────────────────────────────────────────
ind-progress-determinate-label = 60 %
ind-link-docs = Ouvrir la documentation FernUI
ind-link-handler = Avec un gestionnaire de clic

# ── Inputs tab ──────────────────────────────────────────────────────────
inp-checkbox-two-state = Case à cocher à deux états
inp-checkbox-tristate = Case à cocher à trois états
inp-checkbox-disabled = Désactivée (non basculable)
inp-radio-a = Option A
inp-radio-b = Option B
inp-radio-c = Option C
inp-toggle-feature = Activer la fonctionnalité
inp-toggle-with-label = Avec étiquette
inp-toggle-disabled = Bascule désactivée
inp-slider-volume = Volume
inp-slider-stepped = Qualité (par paliers de 25)
inp-slider-vertical = Curseur vertical
inp-segment-first = Premier
inp-segment-second = Deuxième
inp-segment-third = Troisième
inp-combo-apple = Pomme
inp-combo-banana = Banane
inp-combo-cherry = Cerise
inp-combo-placeholder = Choisir un fruit

# ── Buttons tab ─────────────────────────────────────────────────────────
btn-default = Par défaut
btn-regular = Standard
btn-flat = Plat
btn-confirm-label = Confirmer
btn-cmdlink-signin-title = Connectez-vous à votre compte FernUI
btn-cmdlink-signin-desc = Utilisez vos identifiants existants pour accéder aux projets.
btn-cmdlink-signup-title = Créer un nouveau compte
btn-cmdlink-signup-desc = Gratuit pour usage personnel et open source.
btn-popover-trigger = Ouvrir le popover
btn-popover-title = Contenu du popover
btn-popover-body = Cliquez à l'extérieur pour fermer.
btn-popover-icon-body = Menu d'ajout rapide

# ── Containers tab ──────────────────────────────────────────────────────
cnt-panel-body = Surface Panel avec arrière-plan + bordure pilotés par les rôles
cnt-card-header = En-tête de la carte
cnt-card-body = Une carte a une élévation (ombre), un en-tête optionnel, un contenu, et un pied de page.
cnt-card-footer = pied de page · teinte automatique
cnt-groupbox-title = Notifications
cnt-cb-sounds = Jouer des sons
cnt-cb-banner = Afficher une bannière
cnt-groupheader-title = Titre de section
cnt-groupheader-body = …contenu sous l'en-tête
cnt-accordion-1-title = Afficher les détails
cnt-accordion-1-body = Corps de la première section accordéon.
cnt-accordion-2-title = Avancé
cnt-accordion-2-body = Corps de la deuxième section accordéon.
cnt-toolbox-general = Général
cnt-toolbox-general-body = Préférences générales
cnt-toolbox-editor = Éditeur
cnt-toolbox-editor-body = Paramètres de l'éditeur
cnt-toolbox-privacy = Confidentialité
cnt-toolbox-privacy-body = Paramètres de confidentialité + télémétrie
cnt-split-leading = Volet de gauche
cnt-split-trailing = Volet de droite

# ── Chrome tab ──────────────────────────────────────────────────────────
chr-status = Prêt · 1247 lignes · UTF-8 · Rust
chr-banner-info-title = Info
chr-banner-info-body = Saviez-vous que FernUI gère le RTL ?
chr-banner-success-title = Succès
chr-banner-success-body = Paramètres enregistrés.
chr-banner-warning-title = Avertissement
chr-banner-warning-body = Le disque est plein à 90 %.
chr-banner-error-title = Erreur
chr-banner-error-body = Connexion réseau perdue.
chr-breadcrumb-home = Accueil
chr-breadcrumb-docs = Documents
chr-breadcrumb-fernui = FernUI
chr-breadcrumb-current = widget-catalog
chr-wizard-title = Bienvenue
chr-wizard-step1 = Bienvenue
chr-wizard-step1-body = Étape 1 — bienvenue dans FernUI
chr-wizard-step2 = Configurer
chr-wizard-step2-body = Étape 2 — configurez votre éditeur
chr-wizard-step3 = Terminer
chr-wizard-step3-body = Étape 3 — vous êtes prêt
chr-wizard-trigger = Ouvrir l'assistant

# ── Visuals tab ─────────────────────────────────────────────────────────
vis-text-body = Texte de corps
vis-text-bold = BodyBold
vis-text-small = Petit secondaire
vis-text-tiny = Minuscule désactivé
vis-image-alt-1 = Icône étoile (raster)
vis-image-alt-2 = Icône étoile, masque circulaire
vis-image-alt-3 = Icône étoile, masque carré arrondi
vis-panel-body = Panneau : arrière-plan + bordure + rayon + remplissage

# ── Layout tab ──────────────────────────────────────────────────────────
lay-overlay = surcouche
lay-padding-body = marge intérieure de 16 px sur tous les côtés
lay-fixed-size = 140 × 40
lay-min-size = min 160 × 32
lay-max-size = limité à ≤ 240 × 32, même avec un texte très long à l'intérieur
lay-aspect-label = 16:9
lay-centered = centré
lay-form-label-a = Étiquette A
lay-form-value-a = valeur A
lay-form-label-b = Étiquette B
lay-form-value-b = valeur B
lay-switcher-next = page suivante

# ── Text tab ────────────────────────────────────────────────────────────
txt-username-label = Nom d'utilisateur
txt-username-placeholder = ex. ferris
txt-readonly-label = Champ en lecture seule
txt-search-placeholder = Tapez un fruit — Apple, Banana, …
txt-file-label = Choisir un fichier
txt-file-placeholder = Aucun fichier sélectionné
txt-input-dialog-trigger = Ouvrir InputDialog
txt-input-dialog-title = Renommer le fichier
txt-input-dialog-prompt = Entrez le nouveau nom du fichier :
txt-input-dialog-placeholder = sans-titre.txt

# ── Color tab ───────────────────────────────────────────────────────────
clr-brand-label = Couleur de marque
clr-accent-label = Accent du thème

# ── Palette tab ─────────────────────────────────────────────────────────
pal-surfaces = Surfaces
pal-text = Texte
pal-editor = Éditeur

# ── Menus tab ───────────────────────────────────────────────────────────
mnu-file = Fichier
mnu-menu-edit = Édition
mnu-standalone-a = Élément autonome A
mnu-with-shortcut = Avec raccourci
mnu-disabled = Élément désactivé

# ── Overlays tab ────────────────────────────────────────────────────────
ovr-tooltip-hover = Survolez-moi
ovr-tooltip-hover-body = Texte d'infobulle simple
ovr-tooltip-longer = Avec un texte plus long
ovr-tooltip-longer-body = Les infobulles peuvent s'étendre sur plusieurs lignes si nécessaire.
ovr-popover-anchor = Ancre
ovr-popover-title = Contenu du popover
ovr-popover-body = Cliquez à l'extérieur pour fermer.
ovr-dialog-trigger = Ouvrir le dialogue
ovr-dialog-title = Exemple de dialogue
ovr-dialog-body = Ceci est un dialogue (présenté via MessageBox::information).
ovr-mb-info = Information
ovr-mb-info-body = Dialogue informationnel.
ovr-mb-warning = Avertissement
ovr-mb-warning-body = Le disque est presque plein.
ovr-mb-error = Erreur
ovr-mb-error-body = Quelque chose a mal tourné.
ovr-mb-confirm = Êtes-vous sûr ?
ovr-mb-confirm-body = Cette action est irréversible.
ovr-snackbar-trigger = Afficher le snackbar
ovr-snackbar-body = Fichier enregistré avec succès
ovr-shadow-body = Surface de type carte avec l'ombre Panel par défaut

# ── Data tab ────────────────────────────────────────────────────────────
dat-fruit-apple = Pomme
dat-fruit-banana = Banane
dat-fruit-cherry = Cerise
dat-fruit-date = Datte
dat-list-row = Ligne
dat-list-item-1 = Premier élément
dat-list-item-2 = Deuxième élément
dat-list-item-3 = Troisième élément
dat-tree-root = Racine
dat-tree-child-a = Enfant A
dat-tree-child-b = Enfant B
dat-tree-grandchild = Petit-enfant
dat-tree-note = TreeView nécessite un TreeModel<T>. Voir `cargo run -p tree-table` pour la démonstration complète.
dat-table-note = TableView nécessite des définitions de colonnes et un ListModel. Voir `cargo run -p data-grid` pour une grille 1k×6.
dat-treetable-note = TreeTable combine les colonnes de TableView avec la hiérarchie de TreeView. Voir `cargo run -p tree-table` pour la démo de système de fichiers fictif.

# ── Animations tab ──────────────────────────────────────────────────────
anim-visible = Visible
anim-expanded = Étendu
anim-tip-1 = Astuce 1 — glissez la barre de séparation
anim-tip-2 = Astuce 2 — essayez Ctrl+P
anim-tip-3 = Astuce 3 — F12 ouvre l'inspecteur
anim-crossfade-next = Variante suivante
anim-collapse-body = Contenu pliant
anim-smooth-body = S'anime à la taille intrinsèque de l'enfant à chaque changement.
anim-shake = Secouer
anim-rotate = Pivoter +45°
anim-blur-toggle = Activer/désactiver le flou
anim-blur-body = Contenu sensible — basculer le flou pour révéler

# ── Settings tab ────────────────────────────────────────────────────────
set-privacy-note = Activé via la fonctionnalité cargo `telemetry`. Lancez `cargo run -p telemetry_plausible` ou similaire pour voir l'UI de consentement.

# ── Auto-generated by /tmp/find_literals.py ───────────────
animations-tip-1-drag-the-divider = Tip 1 — drag the divider
animations-tip-2-try-ctrl-p = Tip 2 — try Ctrl+P
animations-tip-3-f12-opens-the-inspector = Tip 3 — F12 opens the inspector
animations-next-variant = Next variant
animations-collapsing-content = Collapsing content
animations-animates-to-its-child-s-intrin = Animates to its child's intrinsic size on every change.
animations-rotate-45 = Rotate +45°
animations-toggle-blur = Toggle blur
animations-sensitive-content-toggle-blur = Sensitive content — toggle blur to reveal
buttons-save-as = Save As…
data-first-item = First item
data-second-item = Second item
data-third-item = Third item
data-child-a = Child A
data-child-b = Child B
data-treeview-requires-a-treemodel = TreeView requires a TreeModel<T>. See `cargo run -p tree-table` for the full demo.
layout-cross-platform = Cross-platform
layout-clamped-to-240-32-even-with-ve = clamped to ≤ 240 × 32, even with very long text inside
layout-cross-platform-2 = Cross-platform
overlays-hover-me = Hover me
overlays-plain-tooltip-text = Plain tooltip text
overlays-with-longer-text = With longer text
overlays-tooltips-can-wrap-onto-multipl = Tooltips can wrap onto multiple lines if needed.
overlays-popover-content = Popover content
overlays-click-outside-to-dismiss = Click outside to dismiss.
overlays-open-dialog = Open Dialog
overlays-dialog-example = Dialog example
overlays-this-is-a-dialog-presented-via = This is a Dialog (presented via MessageBox::information).
overlays-informational-dialog = Informational dialog.
overlays-disk-is-almost-full = Disk is almost full.
overlays-something-went-wrong = Something went wrong.
overlays-are-you-sure = Are you sure?
overlays-this-action-cannot-be-undone = This action cannot be undone.
overlays-file-saved-successfully = File saved successfully
overlays-file-saved-successfully-2 = File saved successfully
overlays-show-snackbar = Show snackbar
overlays-card-like-surface-with-the-def = Card-like surface with the default Panel shadow
