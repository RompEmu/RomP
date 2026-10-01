#include "rc_api_runtime.h"
#include "rc_api_user.h"
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
static int regions_ready;

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

/* rc_client checks every achievement's addresses while it loads a game, before reporting the load done,
 * so the regions are set up on the first read, for the console of the game being loaded. */
static void prepare_regions(rc_client_t* client) {
  const rc_client_game_t* game = rc_client_get_game_info(client);
  struct retro_memory_map map;
  if (regions_ready || !game || !game->console_id)
    return;
  map.descriptors = map_descriptors;
  map.num_descriptors = map_count;
  rc_libretro_memory_destroy(&regions);
  rc_libretro_memory_init(&regions, has_map ? &map : NULL, core_memory_info, game->console_id);
  regions_ready = 1;
}

static uint32_t read_memory(uint32_t address, uint8_t* buffer, uint32_t num_bytes, rc_client_t* client) {
  prepare_regions(client);
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
  if (result == RC_OK)
    prepare_regions(client);
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
  regions_ready = 0;
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

/* Achievement lists for showing outside the per-frame runtime. */

typedef struct romp_rc_achievement {
  uint32_t id;
  uint32_t points;
  const char* title;
  const char* description;
  const char* badge_url;
  const char* badge_locked_url;
  const char* progress;
  uint32_t unlocked;
} romp_rc_achievement;

typedef void (*romp_rc_request_out)(void* ctx, const char* url, const char* post, const char* content_type);
typedef void (*romp_rc_achievement_out)(void* ctx, const romp_rc_achievement* achievement);
typedef void (*romp_rc_id_out)(void* ctx, uint32_t id);

static int hand_over_request(rc_api_request_t* request, int result, void* ctx, romp_rc_request_out out) {
  if (result == RC_OK)
    out(ctx, request->url, request->post_data, request->content_type);
  rc_api_destroy_request(request);
  return result;
}

int romp_rc_catalog_request(const char* username, const char* token, uint32_t game_id, const char* hash,
    void* ctx, romp_rc_request_out out) {
  rc_api_fetch_game_sets_request_t params;
  rc_api_request_t request;
  memset(&params, 0, sizeof(params));
  params.username = username;
  params.api_token = token;
  params.game_id = game_id;
  params.game_hash = hash;
  return hand_over_request(&request, rc_api_init_fetch_game_sets_request(&request, &params), ctx, out);
}

int romp_rc_unlocks_request(const char* username, const char* token, uint32_t game_id, int hardcore,
    void* ctx, romp_rc_request_out out) {
  rc_api_fetch_user_unlocks_request_t params;
  rc_api_request_t request;
  memset(&params, 0, sizeof(params));
  params.username = username;
  params.api_token = token;
  params.game_id = game_id;
  params.hardcore = hardcore ? 1 : 0;
  return hand_over_request(&request, rc_api_init_fetch_user_unlocks_request(&request, &params), ctx, out);
}

static rc_api_server_response_t server_response(const char* body, size_t length, int status) {
  rc_api_server_response_t response;
  response.body = body;
  response.body_length = length;
  response.http_status_code = status;
  return response;
}

/* Lists the game's official achievements; returns the game's id, or 0 when the answer was not usable. */
uint32_t romp_rc_parse_catalog(const char* body, size_t length, int status, void* ctx, romp_rc_achievement_out out) {
  rc_api_fetch_game_sets_response_t response;
  rc_api_server_response_t server = server_response(body, length, status);
  uint32_t game_id = 0;
  uint32_t s, a;
  if (rc_api_process_fetch_game_sets_server_response(&response, &server) == RC_OK && response.response.succeeded) {
    game_id = response.id;
    for (s = 0; s < response.num_sets; s++) {
      for (a = 0; a < response.sets[s].num_achievements; a++) {
        const rc_api_achievement_definition_t* def = &response.sets[s].achievements[a];
        romp_rc_achievement out_achievement;
        if (def->category != RC_ACHIEVEMENT_CATEGORY_CORE)
          continue;
        memset(&out_achievement, 0, sizeof(out_achievement));
        out_achievement.id = def->id;
        out_achievement.points = def->points;
        out_achievement.title = def->title;
        out_achievement.description = def->description;
        out_achievement.badge_url = def->badge_url;
        out_achievement.badge_locked_url = def->badge_locked_url;
        out(ctx, &out_achievement);
      }
    }
  }
  rc_api_destroy_fetch_game_sets_response(&response);
  return game_id;
}

int romp_rc_parse_unlocks(const char* body, size_t length, int status, void* ctx, romp_rc_id_out out) {
  rc_api_fetch_user_unlocks_response_t response;
  rc_api_server_response_t server = server_response(body, length, status);
  uint32_t i;
  int result = rc_api_process_fetch_user_unlocks_server_response(&response, &server);
  if (result == RC_OK && response.response.succeeded) {
    for (i = 0; i < response.num_achievement_ids; i++)
      out(ctx, response.achievement_ids[i]);
  } else if (result == RC_OK) {
    result = RC_API_FAILURE;
  }
  rc_api_destroy_fetch_user_unlocks_response(&response);
  return result;
}

/* The running game's achievements as rc_client sees them, earned ones included. */
void romp_rc_list_achievements(rc_client_t* client, void* ctx, romp_rc_achievement_out out) {
  rc_client_achievement_list_t* list = rc_client_create_achievement_list(client,
      RC_CLIENT_ACHIEVEMENT_CATEGORY_CORE, RC_CLIENT_ACHIEVEMENT_LIST_GROUPING_LOCK_STATE);
  uint32_t b, a;
  if (!list)
    return;
  for (b = 0; b < list->num_buckets; b++) {
    for (a = 0; a < list->buckets[b].num_achievements; a++) {
      const rc_client_achievement_t* achievement = list->buckets[b].achievements[a];
      romp_rc_achievement out_achievement;
      memset(&out_achievement, 0, sizeof(out_achievement));
      out_achievement.id = achievement->id;
      out_achievement.points = achievement->points;
      out_achievement.title = achievement->title;
      out_achievement.description = achievement->description;
      out_achievement.badge_url = achievement->badge_url;
      out_achievement.badge_locked_url = achievement->badge_locked_url;
      out_achievement.progress = achievement->measured_progress;
      out_achievement.unlocked = achievement->unlocked != 0;
      out(ctx, &out_achievement);
    }
  }
  rc_client_destroy_achievement_list(list);
}

/* Hardcore and progress kept with save states. */

int romp_rc_hardcore(rc_client_t* client) {
  return rc_client_get_hardcore_enabled(client);
}

void romp_rc_set_hardcore(rc_client_t* client, int enabled) {
  rc_client_set_hardcore_enabled(client, enabled);
}

uint32_t romp_rc_console(rc_client_t* client) {
  const rc_client_game_t* game = rc_client_get_game_info(client);
  return game ? game->console_id : 0;
}

size_t romp_rc_progress_size(rc_client_t* client) {
  return rc_client_progress_size(client);
}

int romp_rc_serialize_progress(rc_client_t* client, uint8_t* buffer, size_t size) {
  return rc_client_serialize_progress_sized(client, buffer, size);
}

int romp_rc_deserialize_progress(rc_client_t* client, const uint8_t* buffer, size_t size) {
  return rc_client_deserialize_progress_sized(client, buffer, size);
}

int romp_rc_setting_allowed(const char* library_name, const char* key, const char* value) {
  const rc_disallowed_setting_t* disallowed = rc_libretro_get_disallowed_settings(library_name);
  return disallowed ? rc_libretro_is_setting_allowed(disallowed, key, value) : 1;
}

int romp_rc_system_allowed(const char* library_name, uint32_t console_id) {
  return rc_libretro_is_system_allowed(library_name, console_id);
}

/* Unlocks sent again after RetroAchievements could not be reached. */

int romp_rc_award_request(const char* username, const char* token, uint32_t achievement_id, int hardcore,
    const char* hash, uint32_t seconds_since_unlock, void* ctx, romp_rc_request_out out) {
  rc_api_award_achievement_request_t params;
  rc_api_request_t request;
  memset(&params, 0, sizeof(params));
  params.username = username;
  params.api_token = token;
  params.achievement_id = achievement_id;
  params.hardcore = hardcore ? 1 : 0;
  params.game_hash = hash;
  params.seconds_since_unlock = seconds_since_unlock;
  return hand_over_request(&request, rc_api_init_award_achievement_request(&request, &params), ctx, out);
}

/* 1 when RetroAchievements recorded the unlock, or already had it. */
int romp_rc_award_accepted(const char* body, size_t length, int status) {
  rc_api_award_achievement_response_t response;
  rc_api_server_response_t server = server_response(body, length, status);
  int accepted = rc_api_process_award_achievement_server_response(&response, &server) == RC_OK
      && response.response.succeeded;
  rc_api_destroy_award_achievement_response(&response);
  return accepted;
}
