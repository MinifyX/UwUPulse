// The module: starts the UwUPulse core inside OBS and answers its callbacks
// (log, open a link, save a file, something changed).

#include "plugin.hpp"

#include <atomic>
#include <cstring>

#include <obs-frontend-api.h>
#include <obs-module.h>
#include <util/bmem.h>
#include <util/platform.h>

#include <QApplication>
#include <QDesktopServices>
#include <QDir>
#include <QFileDialog>
#include <QMetaObject>
#include <QStandardPaths>
#include <QThread>
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

std::string take(char *s)
{
	if (!s)
		return {};
	std::string out(s);
	uwupulse_obs_free_string(s);
	return out;
}

// Runs f on the Qt main thread and waits for it.
template<typename F> void on_ui_thread_blocking(F &&f)
{
	QCoreApplication *app = QCoreApplication::instance();
	if (!app || QThread::currentThread() == app->thread()) {
		f();
		return;
	}
	QMetaObject::invokeMethod(app, std::forward<F>(f), Qt::BlockingQueuedConnection);
}

void on_log(void *, int level, const char *msg)
{
	blog(level, "%s", msg);
}

bool on_open_url(void *, const char *url)
{
	QUrl target(QString::fromUtf8(url));
	if (!target.isValid())
		return false;
	QCoreApplication *app = QCoreApplication::instance();
	if (!app)
		return false;
	QMetaObject::invokeMethod(app, [target]() { QDesktopServices::openUrl(target); }, Qt::QueuedConnection);
	return true;
}

int on_save_file(void *, const char *file_name, const char *filter_name, const char *extension, char *out,
		 size_t out_len)
{
	QString picked;
	bool have_ui = QCoreApplication::instance() != nullptr;
	if (!have_ui)
		return UWUPULSE_SAVE_FAILED;
	QString name = QString::fromUtf8(file_name);
	QString filter = QString("%1 (*.%2)").arg(QString::fromUtf8(filter_name), QString::fromUtf8(extension));
	on_ui_thread_blocking([&]() {
		auto *parent = static_cast<QWidget *>(obs_frontend_get_main_window());
		QString dir = QStandardPaths::writableLocation(QStandardPaths::DocumentsLocation);
		picked = QFileDialog::getSaveFileName(parent, QString::fromUtf8(obs_module_text("SaveTitle")),
						      QDir(dir).filePath(name), filter);
	});
	if (picked.isEmpty())
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
	if (g_refresh_queued.exchange(true))
		return;
	QCoreApplication *app = QCoreApplication::instance();
	if (!app) {
		g_refresh_queued = false;
		return;
	}
	QMetaObject::invokeMethod(
		app,
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
	uwupulse::start_core();
	uwupulse::source_register();
	uwupulse::dock_register();
	obs_frontend_add_event_callback(uwupulse::on_frontend_event, nullptr);
	return true;
}

void obs_module_unload(void)
{
	obs_frontend_remove_event_callback(uwupulse::on_frontend_event, nullptr);
	uwupulse_obs_stop(uwupulse::g_core);
	uwupulse::g_core = nullptr;
}
