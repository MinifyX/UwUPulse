/*
 * UwUPulse core for the OBS plugin: the C ABI of crates/uwupulse-obs
 * (src/lib.rs). Keep both in step.
 *
 * The hub runs on its own threads. Callbacks arrive on those threads, never
 * on the caller's; ctx and the callbacks must stay usable from any thread
 * until uwupulse_obs_stop() returns. Strings are UTF-8 and NUL-terminated.
 */
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The plugin's port on a fresh install (the desktop app has 8746). */
#define UWUPULSE_OBS_DEFAULT_PORT 8747

/* save_file answers. */
#define UWUPULSE_SAVE_PICKED 1
#define UWUPULSE_SAVE_CANCELLED 0
#define UWUPULSE_SAVE_FAILED (-1)

typedef struct uwupulse_obs uwupulse_obs;

/* level is OBS's: LOG_ERROR (100), LOG_WARNING (200), LOG_INFO (300),
 * LOG_DEBUG (400). msg starts with "[uwupulse] ". */
typedef void (*uwupulse_log_fn)(void *ctx, int level, const char *msg);
/* Opens one of the project's fixed web pages; true when it worked. */
typedef bool (*uwupulse_open_url_fn)(void *ctx, const char *url);
/* A modal save dialog (may block; called from a worker thread). Writes the
 * chosen path, NUL-terminated, into out (out_len bytes) and returns
 * UWUPULSE_SAVE_PICKED, or UWUPULSE_SAVE_CANCELLED / UWUPULSE_SAVE_FAILED.
 * filter_name is "CSV" or "JSON", extension "csv" or "json" (no dot). */
typedef int (*uwupulse_save_file_fn)(void *ctx, const char *file_name, const char *filter_name,
				     const char *extension, char *out, size_t out_len);
/* The dock URL, the port, the token or the widget list changed. */
typedef void (*uwupulse_changed_fn)(void *ctx);

struct uwupulse_obs_config {
	const char *config_dir; /* keys, settings, phones, widgets */
	const char *data_dir;   /* pulse.db */
	const char *ui_dir;     /* the built frontend: index.html, assets/ */
	const char *device_name; /* NULL: "UwUPulse OBS (<host name>)" */
	uint16_t default_port;  /* 0: UWUPULSE_OBS_DEFAULT_PORT */
	int port_override;      /* < 0: from the settings; >= 0: this port */
	bool bonjour;
	bool ble; /* chest straps at this computer */
	void *ctx;
	uwupulse_log_fn log;
	uwupulse_open_url_fn open_url;
	uwupulse_save_file_fn save_file;
	uwupulse_changed_fn changed;
};

/* Starts the hub (reads the config, binds the port). NULL on failure; the
 * reason goes to cfg->log. */
uwupulse_obs *uwupulse_obs_start(const struct uwupulse_obs_config *cfg);
/* Stops the hub and frees it (a few seconds at most). NULL is fine. */
void uwupulse_obs_stop(uwupulse_obs *h);

/* "http://127.0.0.1:<port>/ui/#s=<secret>". Free with
 * uwupulse_obs_free_string. */
char *uwupulse_obs_ui_url(const uwupulse_obs *h);
/* {"port": 8747, "error": null, "widgets": [{"id", "name", "url"}]}; url is
 * the overlay with the token. Free with uwupulse_obs_free_string. */
char *uwupulse_obs_widgets(const uwupulse_obs *h);
/* The overlay URL of widget id with the current port and token; NULL for
 * an invalid id. Free with uwupulse_obs_free_string. */
char *uwupulse_obs_overlay_url(const uwupulse_obs *h, const char *id);
void uwupulse_obs_free_string(char *s);

/* "1.0.0"; static. */
const char *uwupulse_obs_version(void);

#ifdef __cplusplus
}
#endif
