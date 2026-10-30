// The UwUPulse dock: the app's own page in OBS's built-in browser (obs-browser
// panel). Where that browser is missing (Wayland, OBS without obs-browser) the
// dock offers to open the page in the system browser instead.

#include "plugin.hpp"

#include <obs-frontend-api.h>
#include <obs-module.h>
#include <util/platform.h>

#include <browser-panel.hpp>

#include <thread>

#include <QApplication>
#include <QDesktopServices>
#include <QMetaObject>
#include <QDockWidget>
#include <QLabel>
#include <QPushButton>
#include <QUrl>
#include <QVBoxLayout>
#include <QWidget>

namespace uwupulse {

namespace {

constexpr const char *DOCK_ID = "uwupulse";

QWidget *g_container = nullptr; // owned by OBS's dock once added
QCefWidget *g_browser = nullptr;
std::string g_url;
bool g_exiting = false;

void open_in_system_browser()
{
	std::string url = ui_url();
	if (!url.empty())
		QDesktopServices::openUrl(QUrl(QString::fromStdString(url)));
}

void show_fallback(const char *reason_key)
{
	auto *label = new QLabel(QString::fromUtf8(obs_module_text(reason_key)), g_container);
	label->setWordWrap(true);
	label->setAlignment(Qt::AlignCenter);
	auto *button = new QPushButton(QString::fromUtf8(obs_module_text("OpenInBrowser")), g_container);
	button->setEnabled(core() != nullptr);
	QObject::connect(button, &QPushButton::clicked, button, []() { open_in_system_browser(); });
	auto *layout = static_cast<QVBoxLayout *>(g_container->layout());
	layout->addStretch(1);
	layout->addWidget(label);
	layout->addWidget(button, 0, Qt::AlignHCenter);
	layout->addStretch(1);
}

void no_browser()
{
	blog(LOG_INFO, "[uwupulse] no browser panels here, the dock links to the system browser");
	show_fallback("NoBrowserDock");
}

void create_browser(QCef *cef)
{
	if (g_exiting || !g_container || g_browser)
		return;
	if (!cef) {
		no_browser();
		return;
	}
	g_url = ui_url();
	g_browser = cef->create_widget(g_container, g_url);
	if (!g_browser) {
		no_browser();
		return;
	}
	g_container->layout()->addWidget(g_browser);
}

// Shows the dock on the first start, so it is found; after that it stays
// where people put it (OBS remembers).
void show_first_time()
{
	char *marker = obs_module_config_path("dock-shown");
	if (!marker)
		return;
	if (!os_file_exists(marker)) {
		dock_show();
		os_quick_write_utf8_file(marker, "1\n", 2, false);
	}
	bfree(marker);
}

} // namespace

void dock_register()
{
	g_container = new QWidget();
	g_container->setObjectName("uwupulseDock");
	g_container->setMinimumSize(320, 240);
	auto *layout = new QVBoxLayout(g_container);
	layout->setContentsMargins(0, 0, 0, 0);
	if (!obs_frontend_add_dock_by_id(DOCK_ID, obs_module_text("UwUPulse"), g_container)) {
		blog(LOG_WARNING, "[uwupulse] could not add the dock");
		delete g_container;
		g_container = nullptr;
	}
}

void dock_frontend_loaded()
{
	if (!g_container || g_browser)
		return;
	show_first_time();
	if (!core()) {
		show_fallback("CoreFailed");
		return;
	}
	// Leaked on purpose: obs-browser owns the CEF side and outlives us.
	QCef *cef = obs_browser_init_panel();
	if (!cef) {
		no_browser();
		return;
	}
	// init_browser() starts CEF unless it runs already (false then is no
	// error); waiting for it happens off the UI thread, as OBS does it.
	cef->init_browser();
	std::thread([cef]() {
		bool ready = cef->wait_for_browser_init();
		QMetaObject::invokeMethod(
			qApp, [cef, ready]() { create_browser(ready ? cef : nullptr); }, Qt::QueuedConnection);
	}).detach();
}

void dock_exit()
{
	g_exiting = true;
	if (g_browser)
		g_browser->closeBrowser();
}

void dock_refresh()
{
	if (g_exiting || !g_browser)
		return;
	std::string url = ui_url();
	if (url.empty() || url == g_url)
		return;
	g_url = url;
	g_browser->setURL(url);
}

void dock_show()
{
	if (!g_container)
		return;
	QWidget *w = g_container;
	while (w && !qobject_cast<QDockWidget *>(w))
		w = w->parentWidget();
	auto *dock = qobject_cast<QDockWidget *>(w);
	if (!dock)
		return;
	dock->setVisible(true);
	dock->raise();
	dock->activateWindow();
}

} // namespace uwupulse
