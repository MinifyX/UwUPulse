// UwUPulse in OBS: the module (plugin.cpp), the dock (dock.cpp) and the
// overlay source (source.cpp) around the Rust core (crates/uwupulse-obs).
#pragma once

#include <string>

#include <uwupulse_obs.h>

namespace uwupulse {

// The running core, or nullptr when it failed to start. Set in
// obs_module_load, cleared in obs_module_unload.
uwupulse_obs *core();

// The dock page ("http://127.0.0.1:<port>/ui/#s=..."); empty without a core.
std::string ui_url();
// The page for the system browser with a one-time ticket instead of the
// secret ("http://127.0.0.1:<port>/ui/#t=..."); empty without a core.
std::string ui_ticket_url();
// The overlay URL of a widget with the current port and token; empty for an
// unknown id or without a core.
std::string overlay_url(const char *widget_id);

// Dock: registered at load, browser created once the frontend has loaded.
void dock_register();
void dock_frontend_loaded();
void dock_exit();
void dock_refresh(); // the URL may have changed (UI thread)
void dock_show();    // shows and raises the dock (UI thread)

// Source type "uwupulse_overlay".
void source_register();
void sources_refresh(); // port, token or widgets changed (UI thread)

} // namespace uwupulse
