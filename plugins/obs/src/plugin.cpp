// The module: starts the UwUPulse core inside OBS and answers its callbacks
// (log, open a link, save a file, something changed).

#include "plugin.hpp"

#include <atomic>
#include <condition_variable>
#include <cstring>
#include <memory>
#include <mutex>

#include <obs-frontend-api.h>
#include <obs-module.h>
#include <util/bmem.h>
#include <util/platform.h>

#include <QApplication>
#include <QDesktopServices>
#include <QDir>
#include <QFileDialog>
#include <QMetaObject>
#include <QObject>
#include <QStandardPaths>
#include <QUrl>
#include <QWidget>

OBS_DECLARE_MODULE()
OBS_MODULE_USE_DEFAULT_LOCALE("uwupulse", "en-US")

MODULE_EXPORT const char *obs_module_name(void)
{
	return "UwUPulse";
}

MODULE_EXPORT const char *obs_module_description(void)
{
	return "Heart rate from Apple Watch and chest straps, straight into OBS.";
}

namespace uwupulse {

namespace {

uwupulse_obs *g_core = nullptr;
// One pending refresh at a time: the core may report several changes at once.
std::atomic<bool> g_refresh_queued{false};

// Receiver of everything the core's threads post to the UI thread. Deleted
// at unload, which drops whatever is still queued: no posted call runs after
// the module is gone.
QObject *g_ui_ctx = nullptr;

// Set at unload, before the core stops: save dialogs aren't opened any
// more, and a core thread waiting for one returns at once.
std::atomic<bool> g_unloading{false};
std::mutex g_save_mutex;
std::condition_variable g_save_cv;

std::string take(char *s)
{
	if (!s)
		return {};
	std::string out(s);
	uwupulse_obs_free_string(s);
	return out;
}

void on_log(void *, int level, const char *msg)
{
	blog(level, "%s", msg);
}

bool on_open_url(void *, const char *url)
{
	// Only web pages: the core asks for its project pages, never for local
	// files or other programs' URL schemes.
	QUrl target(QString::fromUtf8(url), QUrl::StrictMode);
	if (!target.isValid() || target.scheme() != QLatin1String("https") || target.host().isEmpty())
		return false;
	if (!g_ui_ctx || g_unloading)
		return false;
	QMetaObject::invokeMethod(g_ui_ctx, [target]() { QDesktopServices::openUrl(target); }, Qt::QueuedConnection);
	return true;
}

// One save dialog request, shared by the core thread that waits and the UI
// thread that shows it (either may be gone first).
struct SaveRequest {
	bool done = false;
	QString picked;
};

int on_save_file(void *, const char *file_name, const char *filter_name, const char *extension, char *out,
		 size_t out_len)
{
	if (!g_ui_ctx || g_unloading)
		return UWUPULSE_SAVE_CANCELLED;
	QString name = QString::fromUtf8(file_name);
	QString filter = QString("%1 (*.%2)").arg(QString::fromUtf8(filter_name), QString::fromUtf8(extension));
	auto request = std::make_shared<SaveRequest>();
	auto finish = [request](QString picked) {
		{
			std::lock_guard<std::mutex> lock(g_save_mutex);
			request->picked = std::move(picked);
			request->done = true;
		}
		g_save_cv.notify_all();
	};
	// Not BlockingQueuedConnection: this thread must be able to give up
	// when the module unloads while the UI thread is busy.
	QMetaObject::invokeMethod(
		g_ui_ctx,
		[name, filter, finish]() {
			if (g_unloading) {
				finish(QString());
				return;
			}
			auto *parent = static_cast<QWidget *>(obs_frontend_get_main_window());
			QString dir = QStandardPaths::writableLocation(QStandardPaths::DocumentsLocation);
			finish(QFileDialog::getSaveFileName(parent, QString::fromUtf8(obs_module_text("SaveTitle")),
							    QDir(dir).filePath(name), filter));
		},
		Qt::QueuedConnection);
	QString picked;
	{
		std::unique_lock<std::mutex> lock(g_save_mutex);
		g_save_cv.wait(lock, [&]() { return request->done || g_unloading; });
		if (!request->done)
			return UWUPULSE_SAVE_CANCELLED;
		picked = request->picked;
	}
	if (picked.isEmpty() || g_unloading)
		return UWUPULSE_SAVE_CANCELLED;
	QByteArray path = QDir::toNativeSeparators(picked).toUtf8();
	if (static_cast<size_t>(path.size()) + 1 > out_len)
		return UWUPULSE_SAVE_FAILED;
	memcpy(out, path.constData(), static_cast<size_t>(path.size()));
	out[path.size()] = '\0';
	return UWUPULSE_SAVE_PICKED;
}

void on_changed(void *)
{
	if (g_unloading || g_refresh_queued.exchange(true))
		return;
	if (!g_ui_ctx) {
		g_refresh_queued = false;
		return;
	}
	QMetaObject::invokeMethod(
		g_ui_ctx,
		[]() {
			g_refresh_queued = false;
			dock_refresh();
			sources_refresh();
		},
		Qt::QueuedConnection);
}

void on_frontend_event(enum obs_frontend_event event, void *)
{
	switch (event) {
	case OBS_FRONTEND_EVENT_FINISHED_LOADING:
		dock_frontend_loaded();
		break;
	case OBS_FRONTEND_EVENT_EXIT:
		dock_exit();
		break;
	default:
		break;
	}
}

void start_core()
{
	// The plugin is a desktop of its own: its own keys, phones and widgets
	// next to the UwUPulse app, in OBS's plugin config folder.
	char *config = obs_module_config_path("");
	const char *data = obs_get_module_data_path(obs_current_module());
	if (!config || !data) {
		blog(LOG_ERROR, "[uwupulse] no config or data folder");
		bfree(config);
		return;
	}
	os_mkdirs(config);
	std::string ui_dir = std::string(data) + "/ui";

	uwupulse_obs_config cfg{};
	cfg.config_dir = config;
	cfg.data_dir = config;
	cfg.ui_dir = ui_dir.c_str();
	cfg.device_name = nullptr;
	cfg.default_port = UWUPULSE_OBS_DEFAULT_PORT;
	cfg.port_override = -1;
	cfg.bonjour = true;
#ifdef __APPLE__
	// OBS.app has no Bluetooth usage description: macOS would end OBS on
	// the first CoreBluetooth call.
	cfg.ble = false;
#else
	cfg.ble = true;
#endif
	cfg.ctx = nullptr;
	cfg.log = on_log;
	cfg.open_url = on_open_url;
	cfg.save_file = on_save_file;
	cfg.changed = on_changed;

	g_core = uwupulse_obs_start(&cfg);
	bfree(config);
}

} // namespace

uwupulse_obs *core()
{
	return g_core;
}

std::string ui_url()
{
	return g_core ? take(uwupulse_obs_ui_url(g_core)) : std::string();
}

std::string ui_ticket_url()
{
	return g_core ? take(uwupulse_obs_ui_ticket_url(g_core)) : std::string();
}

std::string overlay_url(const char *widget_id)
{
	if (!g_core || !widget_id || !*widget_id)
		return {};
	return take(uwupulse_obs_overlay_url(g_core, widget_id));
}

} // namespace uwupulse

bool obs_module_load(void)
{
	blog(LOG_INFO, "[uwupulse] UwUPulse %s", uwupulse_obs_version());
	// Modules load on the UI thread: the context lives there.
	if (QCoreApplication::instance())
		uwupulse::g_ui_ctx = new QObject();
	uwupulse::start_core();
	uwupulse::source_register();
	uwupulse::dock_register();
	obs_frontend_add_event_callback(uwupulse::on_frontend_event, nullptr);
	return true;
}

void obs_module_unload(void)
{
	obs_frontend_remove_event_callback(uwupulse::on_frontend_event, nullptr);
	// First let every core thread waiting for a save dialog go, so stopping
	// doesn't wait on the UI thread (which is this one).
	{
		std::lock_guard<std::mutex> lock(uwupulse::g_save_mutex);
		uwupulse::g_unloading = true;
	}
	uwupulse::g_save_cv.notify_all();
	uwupulse_obs_stop(uwupulse::g_core);
	uwupulse::g_core = nullptr;
	// Drops calls still queued for the UI thread.
	delete uwupulse::g_ui_ctx;
	uwupulse::g_ui_ctx = nullptr;
}
