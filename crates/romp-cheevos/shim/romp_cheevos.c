#include "rc_client.h"
#include "rc_libretro.h"

#include <stdlib.h>
#include <string.h>

/* The fields of an rc_client event that Romp shows, flattened so Rust needs no struct layouts. */
typedef struct romp_rc_event {
  uint32_t type;
  uint32_t id;
  uint32_t points;
  const char* title;
  const char* description;
  const char* badge_url;
  const char* progress;
  const char* tracker;
  const char* error;
} romp_rc_event;

/* Implemented in Rust. `host` is the pointer given to romp_rc_create. */
extern void romp_rc_on_request(void* host, const char* url, const char* post_data,
    const char* content_type, rc_client_server_callback_t callback, void* callback_data);
extern void romp_rc_on_event(void* host, const romp_rc_event* event);
extern void romp_rc_on_done(void* host, int what, int result, const char* error);

typedef void* (*romp_get_memory_data)(unsigned id);
typedef size_t (*romp_get_memory_size)(unsigned id);

static struct retro_memory_descriptor* map_descriptors;
static unsigned map_count;
static int has_map;
static romp_get_memory_data get_data;
static romp_get_memory_size get_size;
static rc_libretro_memory_regions_t regions;

static void free_map(void) {
  unsigned i;
  for (i = 0; i < map_count; i++)
    free((void*)map_descriptors[i].addrspace);
  free(map_descriptors);
  map_descriptors = NULL;
  map_count = 0;
  has_map = 0;
}

/* Cores may free their memory map after announcing it, so a deep copy is kept. */
void romp_rc_set_memory_map(const struct retro_memory_map* map) {
  unsigned i;
  free_map();
  if (!map || !map->descriptors || !map->num_descriptors)
    return;
  map_descriptors = (struct retro_memory_descriptor*)calloc(map->num_descriptors, sizeof(*map_descriptors));
  if (!map_descriptors)
    return;
  memcpy(map_descriptors, map->descriptors, map->num_descriptors * sizeof(*map_descriptors));
  for (i = 0; i < map->num_descriptors; i++) {
    const char* space = map->descriptors[i].addrspace;
    map_descriptors[i].addrspace = space ? strdup(space) : NULL;
  }
  map_count = map->num_descriptors;
  has_map = 1;
}

void romp_rc_set_core_memory(romp_get_memory_data data, romp_get_memory_size size) {
  get_data = data;
  get_size = size;
}

static void core_memory_info(uint32_t id, rc_libretro_core_memory_info_t* info) {
  info->data = get_data ? (uint8_t*)get_data(id) : NULL;
  info->size = get_size ? get_size(id) : 0;
}

static uint32_t read_memory(uint32_t address, uint8_t* buffer, uint32_t num_bytes, rc_client_t* client) {
  (void)client;
  return rc_libretro_memory_read(&regions, address, buffer, num_bytes);
}

static void server_call(const rc_api_request_t* request, rc_client_server_callback_t callback,
    void* callback_data, rc_client_t* client) {
  romp_rc_on_request(rc_client_get_userdata(client), request->url, request->post_data,
      request->content_type, callback, callback_data);
}

static void event_handler(const rc_client_event_t* event, rc_client_t* client) {
  romp_rc_event out;
  memset(&out, 0, sizeof(out));
  out.type = event->type;
  if (event->achievement) {
    out.id = event->achievement->id;
    out.points = event->achievement->points;
    out.title = event->achievement->title;
    out.description = event->achievement->description;
    out.badge_url = event->achievement->badge_url;
    out.progress = event->achievement->measured_progress;
  }
  if (event->leaderboard) {
    out.id = event->leaderboard->id;
    out.title = event->leaderboard->title;
    out.description = event->leaderboard->description;
    out.tracker = event->leaderboard->tracker_value;
  }
  if (event->leaderboard_tracker) {
    out.id = event->leaderboard_tracker->id;
    out.tracker = event->leaderboard_tracker->display;
  }
  if (event->server_error)
    out.error = event->server_error->error_message;
  romp_rc_on_event(rc_client_get_userdata(client), &out);
}

static void login_done(int result, const char* error, rc_client_t* client, void* userdata) {
  (void)userdata;
  romp_rc_on_done(rc_client_get_userdata(client), 0, result, error);
}

static void load_done(int result, const char* error, rc_client_t* client, void* userdata) {
  (void)userdata;
  if (result == RC_OK) {
    const rc_client_game_t* game = rc_client_get_game_info(client);
    struct retro_memory_map map;
    map.descriptors = map_descriptors;
    map.num_descriptors = map_count;
    rc_libretro_memory_destroy(&regions);
    rc_libretro_memory_init(&regions, has_map ? &map : NULL, core_memory_info, game ? game->console_id : 0);
  }
  romp_rc_on_done(rc_client_get_userdata(client), 1, result, error);
}

rc_client_t* romp_rc_create(void* host, int hardcore) {
  rc_client_t* client = rc_client_create(read_memory, server_call);
  if (!client)
    return NULL;
  rc_client_set_userdata(client, host);
  rc_client_set_event_handler(client, event_handler);
  rc_client_set_hardcore_enabled(client, hardcore);
  return client;
}

void romp_rc_destroy(rc_client_t* client) {
  rc_client_destroy(client);
  rc_libretro_memory_destroy(&regions);
}

void romp_rc_login(rc_client_t* client, const char* username, const char* token) {
  rc_client_begin_login_with_token(client, username, token, login_done, NULL);
}

void romp_rc_load_game(rc_client_t* client, uint32_t console_id, const char* path, const uint8_t* data, size_t size) {
  rc_client_begin_identify_and_load_game(client, console_id, path, data, size, load_done, NULL);
}

void romp_rc_load_hash(rc_client_t* client, const char* hash) {
  rc_client_begin_load_game(client, hash, load_done, NULL);
}

void romp_rc_respond(rc_client_server_callback_t callback, void* callback_data, const char* body,
    size_t body_length, int status) {
  rc_api_server_response_t response;
  response.body = body;
  response.body_length = body_length;
  response.http_status_code = status;
  callback(&response, callback_data);
}

/* The loaded game's id, title and achievement count; returns 0 when no game is loaded. */
uint32_t romp_rc_game(rc_client_t* client, const char** title, uint32_t* achievements) {
  const rc_client_game_t* game = rc_client_get_game_info(client);
  rc_client_user_game_summary_t summary;
  if (!game || !game->id)
    return 0;
  *title = game->title;
  rc_client_get_user_game_summary(client, &summary);
  *achievements = summary.num_core_achievements;
  return game->id;
}

size_t romp_rc_user_agent_clause(rc_client_t* client, char* buffer, size_t size) {
  return rc_client_get_user_agent_clause(client, buffer, size);
}
