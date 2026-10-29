#ifndef PET_CORE_H
#define PET_CORE_H
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
typedef struct PetCore PetCore;
PetCore *pet_create(const char *dir);
void pet_destroy(PetCore *core);
const char *pet_settings(PetCore *core);
const char *pet_error(PetCore *core);
bool pet_apply(PetCore *core, const char *json);
bool pet_preview(PetCore *core, const char *json);
bool pet_import(PetCore *core, const char *path);
bool pet_import_profile(PetCore *core, const char *skin_path, const char *cape_path, const char *name, const char *model);
bool pet_reset(PetCore *core);
bool pet_restore_skin(PetCore *core);
void pet_key(PetCore *core, uint32_t key, bool down, float now);
void pet_clear_keys(PetCore *core);
void pet_click(PetCore *core, float now);
const uint8_t *pet_render(PetCore *core, float x, float y, float width, float height, bool shift, float now, float dt);
bool pet_hit(PetCore *core, float x, float y);
void pet_mouse(PetCore *core, uint32_t button, bool over, float now);
void pet_feed(PetCore *core, float now);
void pet_music(PetCore *core, bool playing);
/// kind: 0 wave, 1 bounce, 2 shy, 3 clap. Returns false for an unknown kind.
bool pet_gesture(PetCore *core, uint32_t kind, float now);
bool pet_needs_frames(PetCore *core, float now);
const uint8_t *pet_pixels(PetCore *core);
bool pet_set_glyphs(const uint8_t *bytes, size_t length);
#endif
