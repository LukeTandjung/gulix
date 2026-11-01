# Gulix Tern plugin

Tern's window plugin API can inspect the focused pane, while the Surface Protocol used by `hx` does not report pane blur. This plugin samples the focused pane every 50 ms and forwards a transition away from a `gulix.typst` surface as a TSP `blur` event. Sampling also avoids depending on the relative delivery order of pointer actions and focus notifications.

Install it from the repository root:

```sh
tern plugin install ./tern/gulix --force
```

This copies the package into Tern's plugin directory, so branch switches cannot break the installation. For plugin development, link the checkout instead:

```sh
tern plugin link "$PWD/tern/gulix"
```

A linked plugin tracks the working tree, including when a branch removes or changes the package. The package can also be installed directly from GitHub:

```sh
tern plugin install github.com/LukeTandjung/gulix/tern/gulix
```

`hx` handles the event by deactivating the edited fragment and restoring its current rendered artifact. Within-surface clicks continue to use Gulix's own focus messages.

The plugin leaves Tern's horizontal terminal inset untouched, so Gulix keeps the same left and right padding whether focused or blurred. At startup it clears the `layout` stylesheet installed by older Gulix versions, which removed that inset.
