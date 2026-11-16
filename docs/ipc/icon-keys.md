# Icon keys

`DockItem.iconKey` is opaque except for the `shell:` namespace, which the frontend renders from
built-in SVG:

| Key                         | Meaning                                            |
|-----------------------------|----------------------------------------------------|
| `shell:start`               | launcher / Start                                   |
| `shell:task-view`           | Task View                                          |
| `shell:show-desktop`        | Show Desktop                                       |
| `shell:quick-settings`      | Quick Settings (v1.21)                             |
| `shell:notification-center` | Notification Center (v1.21)                        |
| `shell:volume`              | volume face, drawn from `DockState.volume` (v1.30) |
| `shell:battery`             | battery face, drawn from `DockState.power` (v1.22) |
| `shell:clock`               | clock face, drawn from the page's clock (v1.22)    |
| `shell:recycle-bin-empty`   | Recycle Bin, empty                                 |
| `shell:recycle-bin-full`    | Recycle Bin, has items                             |

Every other key goes through `get_icon(iconKey, size)`. The UI caches the results per key and
size, and requests the size it renders at × the device pixel ratio.
