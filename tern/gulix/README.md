# Gulix Tern plugin

Tern's window plugin API can inspect the focused pane, while the Surface Protocol used by `hxt` does not report pane blur. This plugin samples the focused pane every 50 ms and forwards a transition away from a `gulix.typst` surface as a TSP `blur` event. Sampling also avoids depending on the relative delivery order of pointer actions and focus notifications.

Link it into Tern from the repository root:

```sh
tern plugin link ./tern/gulix
```

`hxt` handles the event by deactivating the edited fragment and restoring its current rendered artifact. Within-surface clicks continue to use Gulix's own focus messages.

The plugin leaves Tern's horizontal terminal inset untouched, so Gulix keeps the same left and right padding whether focused or blurred. At startup it clears the `layout` stylesheet installed by older Gulix versions, which removed that inset.
