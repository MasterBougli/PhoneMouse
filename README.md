# PhoneMouse

Le tactile du téléphone sert de trackpad pour la souris du PC, sur le réseau local.

Le dépôt est public pour que le code puisse être lu. Il n'est pas open source : la [licence](LICENSE) réserve tous les droits. Elle n'autorise ni la modification, ni la redistribution, ni l'usage commercial.

## Lancer

Il faut Windows. Le PC et le téléphone doivent être sur le même réseau.

1. Lancer `PhoneMouse.exe`.
2. Sur le téléphone, ouvrir l'adresse affichée dans la fenêtre.
3. Entrer le code à 4 chiffres.

La fenêtre peut être réduite : le programme continue. La fermer l'arrête.

Si Windows demande une autorisation du pare-feu, accepter le réseau privé.

```text
PhoneMouse.exe [port] [--new-pin]
```

Le port par défaut est `8787`. `--new-pin` remplace le code. Le code en cours est aussi enregistré dans `phonemouse-pin.txt`, à côté de l'exécutable.

## Gestes

- Un doigt déplace le curseur. Une tape fait un clic gauche.
- Deux doigts défilent. Une tape à deux doigts fait un clic droit.
- Un appui long, ou le bouton **Tenir**, maintient le clic.
- Les boutons **Clic** et **Droit** sont en bas de la page. Le curseur **Vitesse** règle la sensibilité.

## Compiler

[Rust](https://www.rust-lang.org/) stable, puis :

```powershell
cargo build --release
```

L'exécutable est `target\release\PhoneMouse.exe`.

## Licence

Copyright (c) 2026 Bougli. Tous droits réservés.

Lire le code sur ce dépôt est la seule utilisation prévue. Toute copie, modification, distribution ou exploitation commerciale exige une autorisation écrite de l'auteur. Le texte complet est dans [LICENSE](LICENSE).
