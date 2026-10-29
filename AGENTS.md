# Installation et espace disque

- Le Mac de l’utilisateur dispose de peu d’espace : ne pas conserver d’anciennes versions de Tasks, de sauvegardes `.app`, d’archives d’installation ou de builds obsolètes sans demande explicite.
- Conserver uniquement l’application installée courante. Une copie temporaire utilisée pour un remplacement doit être supprimée dès que la nouvelle installation est vérifiée.
- Après une compilation et une installation réussies et vérifiées, supprimer les sorties régénérables `target/` et `dist/`, ainsi que les anciennes traces volumineuses des tests devenues inutiles. Ne pas nettoyer pendant une compilation ou un test en cours.
- Préserver le code source, les changements des autres tâches, les dépendances utiles au développement et les données utilisateur. La base dans `~/Library/Application Support/com.dwenn.tasks/` ne fait pas partie de ce nettoyage.
