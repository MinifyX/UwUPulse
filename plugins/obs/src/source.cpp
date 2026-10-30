// The "UwUPulse" source: a widget of this OBS's UwUPulse as a browser source
// that follows the port and the token. It draws a private obs-browser source
// with the overlay URL, so it needs obs-browser like a browser source does.

#include "plugin.hpp"

#include <mutex>
#include <set>

#include <obs-module.h>

namespace uwupulse {

namespace {

constexpr const char *SOURCE_ID = "uwupulse_overlay";
constexpr const char *DEFAULT_WIDGET = "classic";
constexpr int DEFAULT_WIDTH = 600;
constexpr int DEFAULT_HEIGHT = 300;

struct Overlay {
	obs_source_t *self;
	obs_source_t *browser; // private browser_source, may be nullptr
	std::string widget;
	std::string url;
	int width;
	int height;
};

// Every live overlay, so a new port or token reaches all of them.
std::mutex g_lock;
std::set<Overlay *> g_overlays;

std::string target_url(const Overlay *o)
{
	std::string url = overlay_url(o->widget.c_str());
	return url.empty() ? "about:blank" : url;
}

// Points the browser at the widget's current URL (with g_lock held).
void apply(Overlay *o)
{
	if (!o->browser)
		return;
	std::string url = target_url(o);
	obs_data_t *s = obs_data_create();
	obs_data_set_string(s, "url", url.c_str());
	obs_data_set_int(s, "width", o->width);
	obs_data_set_int(s, "height", o->height);
	obs_data_set_bool(s, "reroute_audio", false);
	obs_source_update(o->browser, s);
	obs_data_release(s);
	o->url = url;
}

void read_settings(Overlay *o, obs_data_t *settings)
{
	o->widget = obs_data_get_string(settings, "widget");
	o->width = static_cast<int>(obs_data_get_int(settings, "width"));
	o->height = static_cast<int>(obs_data_get_int(settings, "height"));
	if (o->width <= 0)
		o->width = DEFAULT_WIDTH;
	if (o->height <= 0)
		o->height = DEFAULT_HEIGHT;
}

const char *get_name(void *)
{
	return obs_module_text("UwUPulse");
}

void *create(obs_data_t *settings, obs_source_t *self)
{
	auto *o = new Overlay{self, nullptr, {}, {}, DEFAULT_WIDTH, DEFAULT_HEIGHT};
	read_settings(o, settings);

	std::string name = std::string(obs_source_get_name(self)) + " (browser)";
	obs_data_t *s = obs_data_create();
	obs_data_set_string(s, "url", "about:blank");
	obs_data_set_int(s, "width", o->width);
	obs_data_set_int(s, "height", o->height);
	obs_data_set_bool(s, "reroute_audio", false);
	o->browser = obs_source_create_private("browser_source", name.c_str(), s);
	obs_data_release(s);
	if (o->browser)
		obs_source_add_active_child(self, o->browser);
	else
		blog(LOG_WARNING, "[uwupulse] the UwUPulse source needs obs-browser");

	std::lock_guard<std::mutex> lock(g_lock);
	g_overlays.insert(o);
	apply(o);
	return o;
}

void destroy(void *data)
{
	auto *o = static_cast<Overlay *>(data);
	{
		std::lock_guard<std::mutex> lock(g_lock);
		g_overlays.erase(o);
	}
	if (o->browser) {
		obs_source_remove_active_child(o->self, o->browser);
		obs_source_release(o->browser);
	}
	delete o;
}

void update(void *data, obs_data_t *settings)
{
	auto *o = static_cast<Overlay *>(data);
	std::lock_guard<std::mutex> lock(g_lock);
	read_settings(o, settings);
	apply(o);
}

void get_defaults(obs_data_t *settings)
{
	obs_data_set_default_string(settings, "widget", DEFAULT_WIDGET);
	obs_data_set_default_int(settings, "width", DEFAULT_WIDTH);
	obs_data_set_default_int(settings, "height", DEFAULT_HEIGHT);
}

bool open_dock(obs_properties_t *, obs_property_t *, void *)
{
	dock_show();
	return false;
}

obs_properties_t *get_properties(void *data)
{
	auto *o = static_cast<Overlay *>(data);
	obs_properties_t *props = obs_properties_create();
	obs_property_t *list = obs_properties_add_list(props, "widget", obs_module_text("Widget"),
						       OBS_COMBO_TYPE_LIST, OBS_COMBO_FORMAT_STRING);

	bool have_current = false;
	char *json = core() ? uwupulse_obs_widgets(core()) : nullptr;
	if (json) {
		obs_data_t *doc = obs_data_create_from_json(json);
		uwupulse_obs_free_string(json);
		obs_data_array_t *widgets = doc ? obs_data_get_array(doc, "widgets") : nullptr;
		size_t n = widgets ? obs_data_array_count(widgets) : 0;
		for (size_t i = 0; i < n; i++) {
			obs_data_t *w = obs_data_array_item(widgets, i);
			const char *id = obs_data_get_string(w, "id");
			const char *name = obs_data_get_string(w, "name");
			obs_property_list_add_string(list, *name ? name : id, id);
			if (o && o->widget == id)
				have_current = true;
			obs_data_release(w);
		}
		obs_data_array_release(widgets);
		obs_data_release(doc);
	}
	// A widget deleted in the meantime stays selectable, marked as missing.
	if (o && !o->widget.empty() && !have_current) {
		std::string label = o->widget + " (" + obs_module_text("Missing") + ")";
		obs_property_list_add_string(list, label.c_str(), o->widget.c_str());
	}
	if (!core())
		obs_properties_add_text(props, "core_failed", obs_module_text("CoreFailed"), OBS_TEXT_INFO);

	obs_properties_add_int(props, "width", obs_module_text("Width"), 1, 8192, 1);
	obs_properties_add_int(props, "height", obs_module_text("Height"), 1, 8192, 1);
	obs_properties_add_button(props, "open", obs_module_text("OpenUwUPulse"), open_dock);
	return props;
}

uint32_t get_width(void *data)
{
	return static_cast<uint32_t>(static_cast<Overlay *>(data)->width);
}

uint32_t get_height(void *data)
{
	return static_cast<uint32_t>(static_cast<Overlay *>(data)->height);
}

void video_render(void *data, gs_effect_t *)
{
	auto *o = static_cast<Overlay *>(data);
	if (o->browser)
		obs_source_video_render(o->browser);
}

void enum_active_sources(void *data, obs_source_enum_proc_t cb, void *param)
{
	auto *o = static_cast<Overlay *>(data);
	if (o->browser)
		cb(o->self, o->browser, param);
}

void show(void *data)
{
	auto *o = static_cast<Overlay *>(data);
	if (o->browser)
		obs_source_inc_showing(o->browser);
}

void hide(void *data)
{
	auto *o = static_cast<Overlay *>(data);
	if (o->browser)
		obs_source_dec_showing(o->browser);
}

} // namespace

void source_register()
{
	obs_source_info info = {};
	info.id = SOURCE_ID;
	info.type = OBS_SOURCE_TYPE_INPUT;
	info.output_flags = OBS_SOURCE_VIDEO | OBS_SOURCE_CUSTOM_DRAW | OBS_SOURCE_DO_NOT_DUPLICATE;
	info.icon_type = OBS_ICON_TYPE_BROWSER;
	info.get_name = get_name;
	info.create = create;
	info.destroy = destroy;
	info.update = update;
	info.get_defaults = get_defaults;
	info.get_properties = get_properties;
	info.get_width = get_width;
	info.get_height = get_height;
	info.video_render = video_render;
	info.enum_active_sources = enum_active_sources;
	info.show = show;
	info.hide = hide;
	obs_register_source(&info);
	blog(LOG_INFO, "[uwupulse] source type %s registered", SOURCE_ID);
}

void sources_refresh()
{
	std::lock_guard<std::mutex> lock(g_lock);
	for (Overlay *o : g_overlays) {
		if (target_url(o) != o->url)
			apply(o);
	}
}

} // namespace uwupulse
