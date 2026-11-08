# Windows

Each window of the app has a label and loads one page:

| Label              | Page                      | Purpose                         |
|--------------------|---------------------------|---------------------------------|
| `dock`             | `dock.html`               | the dock on the primary monitor |
| `dock-<monitorId>` | `dock.html?monitor=<id>`  | the docks on the other monitors (milestone M3 in the [plan](../PLAN.md)) |
| `settings`         | `settings.html`           | the settings window. It also reads `get_dock_state`, `dock://state` and `get_icon` for its pinned list, and `get_app_info` and `open_app_folder` for its About section. |
