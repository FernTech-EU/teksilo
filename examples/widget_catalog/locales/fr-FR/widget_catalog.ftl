# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# Teksilo Widget Catalog — French translations.

# ── App chrome ──────────────────────────────────────────────────────────
app-title = Teksilo — Catalogue de Widgets
app-subtitle = glisser · double-clic pour agrandir · clic droit pour le menu
app-unsupported-chrome = (chrome personnalisé non pris en charge sur cette plateforme — repli sur les décorations natives)

# ── Barre de menu de l'application ───────────────────────────────────────
app-menu-file = &Fichier
app-menu-help = &Aide
app-menu-quit = &Quitter
app-menu-documentation = &Documentation
app-menu-about = &À propos

# ── View mode toggle ────────────────────────────────────────────────────
mode-label = DSL teksu!
mode-tooltip = Bascule chaque onglet entre la version classique (constructeur) et la version macro teksu! du même arbre.

# ── Locale switcher ─────────────────────────────────────────────────────
locale-en = English
locale-fr = Français
locale-ar = العربية

# ── Theme switcher ──────────────────────────────────────────────────────
theme-label = Thème
theme-tooltip = Basculer entre les thèmes clair et sombre.
os-theme-label = Thème système
os-theme-tooltip = Suivre les couleurs du thème du bureau (accent, surfaces, texte).

# ── Tab titles ──────────────────────────────────────────────────────────
tab-palette-title = Palette
tab-layout-title = Disposition
tab-visuals-title = Visuels
tab-containers-title = Conteneurs
tab-chrome-title = Chrome
tab-buttons-title = Boutons
tab-styling-title = Style
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
tab-charts-title = Graphiques
tab-scene-title = Scène
tab-richtext-title = Texte enrichi
tab-dragdrop-title = Glisser-déposer

# ── Deep-dive references ────────────────────────────────────────────────
tab-palette-refs = Tous les rôles de surface, de texte et d'éditeur, avec un pangramme texte enrichi + emojis pour rendre le changement de thème visible. Voir : docs/reactive-theme.md.
tab-layout-refs = Piles, grilles et distribution du jeu. Voir : cargo run -p text_and_layout, cargo run -p split_view.
tab-visuals-refs = Primitives de rendu, icônes, images, bandes de validation. Voir : cargo run -p simple_button.
tab-containers-refs = Panneaux, cartes, accordéons, zones de défilement, vues fractionnées. Voir : cargo run -p split_view, cargo run -p tool_box.
tab-chrome-refs = Chrome de l'application : barre d'outils, barre d'état, fil d'Ariane, assistants, bannières. Voir : cargo run -p title_bar_demo.
tab-buttons-refs = Toutes les variantes de bouton. Voir : cargo run -p simple_button, cargo run -p menus_and_dropdowns.
tab-styling-refs = L'échelle de style à quatre niveaux : grilles de variantes (niveau 1) et surcharges .style(impl FooStyle) par appel (niveau 3). Voir : docs/styling-system.md, cargo run -p theme_styles.
tab-inputs-refs = Saisies booléennes et de sélection. La navigation clavier approfondie est dans les exemples dédiés.
tab-indicators-refs = État en lecture seule : progression, sabliers, badges, avatars, liens.
tab-text-refs = Édition de texte simple ligne et riche. Voir : cargo run -p rich_text_editor, cargo run -p spin_box.
tab-datetime-refs = Calendrier, sélecteurs de date, d'heure et de plage. Voir : cargo run -p datetime_pickers.
tab-color-refs = Saisie hexadécimale, édition compacte, sélecteur HSV complet. Voir : cargo run -p color_picker.
tab-menus-refs = Barre de menu, listes, menus contextuels. Voir : cargo run -p menus_and_dropdowns.
tab-overlays-refs = Infobulles, popovers, dialogues, snackbars. Voir : cargo run -p tooltips_showcase, cargo run -p dialogs_and_popovers, cargo run -p file_dialogs.
tab-data-refs = ListView, TreeView, TableView, TreeTableView. Voir : cargo run -p data_grid, cargo run -p tree_table, cargo run -p data_collections.
tab-animations-refs = Fade, pulse, slide, blur et leurs amis. Voir : cargo run -p animations, cargo run -p animations_kit.
tab-settings-refs = Widgets de réaffectation des raccourcis et de paramètres de confidentialité. Voir : cargo run -p shortcuts_demo.
tab-charts-refs = Graphiques en barres, courbes et anneau (teksilo-charts). Voir : cargo run -p chart_demo.
tab-scene-refs = Vue de scène panoramique / zoomable (teksilo-scene). Voir : cargo run -p scene_showcase, cargo run -p scene_corkboard.
tab-richtext-refs = Texte enrichi éditable et en lecture seule sur le modèle text-document. Voir : cargo run -p rich_text_editor, cargo run -p rich_text_viewer.
tab-dragdrop-refs = DropZone et DropTarget pour les dépôts OS / internes. Voir : cargo run -p file_drop.

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
ind-link-docs = Ouvrir la documentation Teksilo
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
btn-cmdlink-signin-title = Connectez-vous à votre compte Teksilo
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
chr-banner-info-body = Saviez-vous que Teksilo gère le RTL ?
chr-banner-success-title = Succès
chr-banner-success-body = Paramètres enregistrés.
chr-banner-warning-title = Avertissement
chr-banner-warning-body = Le disque est plein à 90 %.
chr-banner-error-title = Erreur
chr-banner-error-body = Connexion réseau perdue.
chr-breadcrumb-home = Accueil
chr-breadcrumb-docs = Documents
chr-breadcrumb-teksilo = Teksilo
chr-breadcrumb-current = widget-catalog
chr-wizard-title = Bienvenue
chr-wizard-step1 = Bienvenue
chr-wizard-step1-body = Étape 1 — bienvenue dans Teksilo
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
lay-column-flow-count = { $n } colonne{ $n ->
        [one] { "" }
       *[other] s
    } — rétrécissez la fenêtre pour voir la redistribution
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
mnu-alignment = Alignement
mnu-align-left = Gauche
mnu-align-center = Centrer
mnu-align-right = Droite

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
dat-treetable-note = TreeTableView combine les colonnes de TableView avec la hiérarchie de TreeView. Voir `cargo run -p tree-table` pour la démo de système de fichiers fictif.

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

# ── Catalog i18n pass: translatable visual strings ──────────────────────
# Tooltip cascade demo (shared.rs)
tip-a-body = Niveau 1 de la cascade. Survolez le [lien suivant](:tip-b) pour ouvrir le niveau 2.
tip-a-more = Ouvrez l'accordéon pour lire ce texte long sans quitter l'infobulle.
tip-b-body = Niveau 2 de la cascade. Survolez le [dernier lien](:tip-c) pour en ouvrir un de plus.
tip-b-more = Chaque infobulle imbriquée rattache sa surcouche à la précédente (OverlayLayer::InTree).
tip-c-body = Niveau 3 — fin de la cascade. Appuyez sur Échap ou cliquez à l'extérieur pour fermer.
tip-stat-food-body = **Nourriture** modifie le taux de croissance de votre population. Lié au [commerce](:stat-trade).
tip-stat-trade-body = **Commerce** : les routes affectent les revenus en pièces. Lié au [bonheur](:stat-happiness).
tip-stat-happiness-body = **Bonheur** limite les troubles. Fin de la cascade interne au composite.
# Indicators
ind-progress-determinate-heading = ProgressBar — déterminée
ind-progress-indeterminate-heading = ProgressBar — indéterminée
ind-progress-vertical-heading = ProgressBar — verticale
# Styling
sty-tier1-button-variant-heading = Niveau 1 — ButtonVariant
sty-tier1-toggle-variant-heading = Niveau 1 — ToggleVariant
sty-tier1-checkbox-variant-heading = Niveau 1 — CheckboxVariant
sty-tier1-card-variant-heading = Niveau 1 — CardVariant
sty-tier3-button-style-heading = Niveau 3 — Button::style(impl ButtonStyle)
sty-tier3-toggle-style-heading = Niveau 3 — Toggle::style(impl ToggleStyle)
# Containers
cnt-scrollbar-standalone-heading = ScrollBar (autonome)
# Layout
lay-above = au-dessus
lay-below = en-dessous
# Data
dat-standard-list-item-standalone = StandardListItem (autonome)
dat-standard-tree-item-standalone = StandardTreeItem (autonome)
# Menus
mnu-menu-list-standalone = MenuList (autonome)
mnu-menu-item-standalone = MenuItem (autonome)
# Visuals
vis-panel-standalone = Panel (exemple de primitive visuelle)
# Date & Time
dt-calendar-single = Calendar — date unique
dt-calendar-range = Calendar — plage de dates
# Text
txt-password-label = Mot de passe
txt-password-placeholder = Entrez votre mot de passe
txt-password-validation = Utilisez au moins 8 caractères
# Buttons
btn-heading-variants = Button — variantes
btn-heading-disabled = Button — état désactivé
btn-heading-with-icon = Button — avec icône
btn-export-sample = Exporter
# Inputs
inp-heading-radio-group = RadioButton (dans un groupe)
inp-heading-slider-h = Slider — horizontal
inp-heading-slider-stepped = Slider — avec pas
inp-heading-slider-v = Slider — vertical
# Overlays
ovr-plain-tooltips-heading = Infobulles simples
ovr-plain-tooltips-subtitle = (une ligne, éphémère)
ovr-tooltip-save-doc = Enregistrer le document actuel
ovr-tooltip-open-file = Ouvrir un fichier
ovr-tooltip-close-tab = Fermer l'onglet
ovr-rich-tooltips-heading = Infobulles enrichies
ovr-rich-tooltips-subtitle = (cascade :key, maintien pour épingler)
ovr-hover-level-1 = Survolez pour le niveau 1
ovr-hover-level-2 = Survolez pour le niveau 2
ovr-hover-level-3 = Survolez pour le niveau 3
ovr-plain-among-rich = Simple parmi les enrichies
ovr-plain-among-rich-tip = Infobulle simple dans la colonne enrichie — diagnostic.
ovr-rich-dwell-tip = Astuce : maintenez ~2 s pour épingler, puis cliquez sur les liens pour enchaîner.
ovr-province-iberia = Iberia
ovr-province-overview = Aperçu de la province
ovr-stat-food-label = Nourriture : 42
ovr-stat-trade-label = Commerce : 18
ovr-stat-happiness-label = Bonheur : 71 %
ovr-tab-stats = Statistiques
ovr-stat-population = Population : 12 400
ovr-stat-garrison = Garnison : 320
ovr-tab-history = Histoire
ovr-province-history = Fondée en 1247 • 3 sièges • 1 épidémie
ovr-tabbed-details = Détails à onglets
ovr-treasury-report = Rapport du trésor
ovr-treasury-subtitle = Ce trimestre : +423 pièces
ovr-open-ledger = Ouvrir le grand livre
ovr-composite-tooltips-heading = Infobulles composites
ovr-composite-tooltips-subtitle = (arbre de widgets arbitraire, style CK3)
ovr-province-info-btn = Infos sur la province
ovr-with-internal-button = Avec Button interne
ovr-composite-dwell-tip = Astuce : maintenez ~2 s, puis Tab dans la surface, puis activez le Button interne.
ovr-section-tooltip-cascade = Tooltip — simple / enrichi / composite (cascade à 3 niveaux)
ovr-section-popover = Popover (autonome)
ovr-section-dialog = Dialog (via MessageBox)
ovr-section-messagebox = MessageBox — variantes de gravité
ovr-section-shadow = Shadow (primitive visuelle)

# ── Toast triggers (Overlays tab) ──────────────────────────────────────
ovr-toast-btn-info = Info
ovr-toast-btn-success = Succès
ovr-toast-btn-warning = Avertissement
ovr-toast-btn-error = Erreur
ovr-toast-btn-loading = Chargement
ovr-toast-info-msg = Pour information
ovr-toast-success-msg = Enregistré
ovr-toast-warning-msg = Avertissement
ovr-toast-warning-body = Jetez-y un œil dès que possible.
ovr-toast-error-msg = Échec de la compilation
ovr-toast-error-body = Trois erreurs, deux avertissements.
ovr-toast-error-action = Voir les erreurs
ovr-toast-loading-msg = Traitement en cours…

# ── Drag & Drop tab ────────────────────────────────────────────────────
dnd-zone-images-title = Déposez des images ici
dnd-zone-images-subtitle = PNG · JPEG · GIF
dnd-zone-any-title = Déposez n'importe quoi ici
dnd-zone-any-subtitle = fichiers, texte ou liens
dnd-target-body = DropTarget — entoure un Panel ; déposez un fichier pour voir la bordure se surligner
dnd-target-hint = Relâchez pour déposer
dnd-log-initial = Les éléments déposés apparaîtront ici.
dnd-section-zone-any = DropZone — fichiers / texte / URL
dnd-section-zone-images = DropZone — images uniquement
dnd-section-target = DropTarget — conteneur englobant
dnd-section-log = Journal des dépôts
