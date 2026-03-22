#include "bindings/bindings.h"

extern "C" char *nvpn_ios_prepare(void);
extern "C" char *nvpn_ios_start(const char *request);
extern "C" char *nvpn_ios_stop(void);
extern "C" char *nvpn_ios_status(void);
extern "C" void nvpn_ios_free_string(char *value);
extern "C" bool nvpn_ios_register_bridge(
	char *(*prepare)(void),
	char *(*start)(const char *),
	char *(*stop)(void),
	char *(*status)(void),
	void (*free_string)(char *)
);

int main(int argc, char * argv[]) {
	nvpn_ios_register_bridge(
		nvpn_ios_prepare,
		nvpn_ios_start,
		nvpn_ios_stop,
		nvpn_ios_status,
		nvpn_ios_free_string
	);
	ffi::start_app();
	return 0;
}
