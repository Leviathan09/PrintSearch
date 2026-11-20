# Historique des versions

[English](CHANGELOG.md) · **Français**

Les changements notables du projet sont consignés ici.
Le format suit [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/) et le projet utilise le [versionnage sémantique](https://semver.org/lang/fr/).

## [0.2.0] - 2026-11-20

### Ajouté
- **Choix du navigateur utilisé pour les recherches.** MakerWorld (et Thingiverse sans jeton) sont lus avec un navigateur basé sur Chromium qui tourne en arrière-plan. Vous pouvez maintenant le choisir dans **⚙ Paramètres → Navigateur utilisé pour les recherches** : *Automatique*, un navigateur détecté (**Chrome, Edge, Brave, Vivaldi, Chromium**) ou un **chemin personnalisé**. Chrome n'est plus obligatoire : **Edge, préinstallé sous Windows, suffit**.
- Commande `printsearch browser` pour lister les navigateurs détectés et en sélectionner un (`auto`, `chrome`, `edge`, `brave`, `vivaldi`, `chromium` ou un chemin).
- **Icône de l'application intégrée à l'`.exe` Windows** (`build.rs`, `packaging/printsearch.ico`). Si l'icône ne peut pas être intégrée (par exemple `windres` absent en compilation croisée), la compilation réussit quand même et affiche un avertissement.
- **Résultats progressifs :** chaque site est interrogé séparément, donc les sites rapides (par exemple Printables) s'affichent tout de suite au lieu d'attendre le plus lent. Un message de progression indique combien de sites ont répondu, et l'erreur d'un site ne masque plus les autres.
- `printsearch --version`.
- Ce changelog, en anglais et en français.

### Modifié
- Les résultats sont fusionnés côté interface (sites entrelacés, puis triés par likes ou téléchargements si demandé). Les cartes déjà affichées sont réutilisées : plus de clignotement, et leur animation d'apparition n'est plus rejouée. Un même modèle présent deux fois n'est affiché qu'une fois.
- Un seul client HTTP est partagé par tout le programme (connexions réutilisées) et il a désormais des délais d'attente : 8 s pour se connecter, 25 s par requête.
- Le cache du lecteur de pages est plafonné (40 entrées) et les entrées expirées sont purgées.
- Le fichier de configuration est écrit de façon atomique (fichier temporaire puis renommage).
- Le paquet `.deb` recommande maintenant aussi Edge et Brave, en plus de Chromium et Chrome.
- Le mainteneur et l'auteur du paquet sont désormais **Print-lab**.
- READMEs mis à jour (choix du navigateur, icône Windows, compilation croisée depuis Linux, `rustfmt` requis pour compiler).

### Sécurité
- Le serveur local rejette toute requête dont le `Host` n'est pas `localhost:8787` / `127.0.0.1:8787`, et toute requête avec une `Origin` étrangère. Cela protège contre les pages web malveillantes ouvertes dans votre navigateur (CSRF et « DNS rebinding »), qui auraient pu appeler l'API locale, par exemple pour changer l'exécutable du navigateur.

### Corrigé
- **Blocage quand le navigateur sélectionné n'est pas un vrai navigateur** (mauvais chemin, programme non basé sur Chromium) : le lancement pouvait bloquer indéfiniment et figer les recherches suivantes. Le lancement échoue maintenant après 35 s avec un message clair, un échec de lancement est mémorisé 20 s au lieu d'être retenté en boucle, chaque appel au navigateur a un délai de 10 s, et la lecture d'une page est plafonnée à 75 s.
- Une recherche lancée sans aucun site sélectionné pouvait afficher les résultats de la recherche précédente.
- Un plantage dans un thread ne peut plus désactiver définitivement le lecteur de pages (les verrous empoisonnés sont récupérés).
- Le démarrage d'une seconde instance indique clairement que le port 8787 est déjà utilisé.

### Remarques
- Firefox et Safari ne peuvent pas servir aux recherches (le lecteur exige un navigateur basé sur Chromium).
- La compilation nécessite `rustfmt` (inclus dans une installation `rustup` normale) à cause de la dépendance `headless_chrome`.
- Limites inchangées : les points d'accès de Printables et MakerWorld sont non officiels, et Like / collections restent expérimentaux (voir le README).

## [0.1.0]

Première version.

### Ajouté
- Recherche sur **Thingiverse, Printables et MakerWorld** depuis une interface web locale et en ligne de commande.
- Filtres (sites, prix, tri, likes et téléchargements minimum, résultats par site) avec relance automatique de la recherche, pagination (« Charger plus ») et animations.
- Actions Like et ajout à une collection (expérimentales), connexion aux sites depuis une page Paramètres (App Token ou OAuth pour Thingiverse, jeton Bearer pour Printables).
- Interface en anglais et en français (anglais par défaut) et historique de recherche désactivable.
- Packaging : métadonnées `.deb`, lanceur et icône ; compilation Windows. Publié sous licence GPL v3.

[0.2.0]: https://github.com/Print-lab/PrintSearch/releases/tag/v0.2.0
[0.1.0]: https://github.com/Print-lab/PrintSearch/releases/tag/v0.1.0
