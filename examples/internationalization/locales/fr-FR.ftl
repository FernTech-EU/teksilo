# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# French runtime translation. Missing keys fall back to en-US at runtime.

window-title = Démo d'internationalisation
-brand = Bastyde
heading = Vitrine i18n de { -brand }
greeting = Bonjour, { $name } !
body-paragraph = Choisissez une langue dans la liste ci-dessous. Passer à l'arabe inverse le sens de la mise en page — le début et la fin s'intervertissent, et la rangée du bas inverse visiblement ses enfants. L'anglais et le français sont tous deux de gauche à droite, la rangée garde donc le même ordre entre eux.
direction-note-ltr = Direction de la mise en page : de gauche à droite
direction-note-rtl = Direction de la mise en page : de droite à gauche
language-label = Langue :
lang-english = Anglais
lang-french = Français
lang-arabic = Arabe
leading-button = Début
trailing-button = Fin
status-en = Affichage en anglais
status-fr = Affichage en français
status-ar = Affichage en arabe

# Vitrine du formatage selon la locale.
formatting-heading = Formatage selon la locale
bundle-currency-row = Total (bundle) : { NUMBER($price, style: "currency", currency: "EUR") }
bundle-date-row = Aujourd'hui (bundle) : { DATETIME($ts, dateStyle: "long") }
cart-summary = { $count } articles à { NUMBER($price) } pièce
price-label = Prix :
count-label = Quantité :
