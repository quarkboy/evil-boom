/* Evil's way into DSDA-Doom: the game run a tic at a time, in-process, nothing drawn or
 * heard, and its state read straight out of memory (no copying the whole world into
 * objects every tic, as ViZDoom does).
 *
 * Built into a shared library with DSDA-Doom's own sources (see dsda/README.md). Plain C
 * types only across the boundary, so any toolchain can load it. One game a process: the
 * engine is all globals. */
#include <stdio.h>
#include <string.h>

#include "SDL.h"

#include "doomstat.h"
#include "d_event.h"
#include "d_main.h"
#include "d_player.h"
#include "g_game.h"
#include "m_menu.h"
#include "m_random.h"
#include "p_mobj.h"
#include "p_map.h"
#include "p_tick.h"
#include "r_state.h"
#include "i_system.h"
#include "i_video.h"
#include "v_video.h"
#include "r_main.h"
#include "r_fps.h"
#include "st_stuff.h"
#include "lprintf.h"
#include "dsda/args.h"
#include "dsda/pause.h"
#include "dsda/configuration.h"

#ifdef _WIN32
#define EVIL_API __declspec(dllexport)
#else
#define EVIL_API __attribute__((visibility("default")))
#endif

/* d_main.c: the game's own setup, without its main loop (added to DSDA-Doom by the patch). */
void D_DoomMainSetupForEvil(void);
extern SDL_Window *sdl_window;
void M_LoadDefaults(void);

typedef struct {
  int id;          /* the thing's own number, from its spawning on (evil_id) */
  int type;        /* mobjtype_t */
  double x, y, z;
  double momx, momy, momz;
  double angle;    /* degrees, 0 = east, counter-clockwise */
  double radius, height;
  int health;
  unsigned long long flags;
  int owner_type;  /* a missile: the type of what fired it (its target), -1: none */
  int state;       /* its state now (an index into states[]: what it's doing) */
  int after_her;   /* a monster: its target is the player */
  int attack;      /* a monster: tics into its (missile) attack, from its first frame; -1: not attacking */
} evil_mobj;

typedef struct {
  double x, y, z, angle, momx, momy, momz;
  int health, armor, readyweapon, pendingweapon;
  int weaponowned[9];  /* by weapontype_t: fist, pistol, shotgun, chaingun, missile, plasma,
                          bfg, chainsaw, supershotgun */
  int ammo[4];         /* clip, shell, cell, missile */
  int cards[6];        /* blue card, yellow card, red card, blue skull, yellow skull, red skull */
  int ironfeet;        /* radiation suit: tics left */
  int playerstate;     /* 0 alive, 1 dead, 2 reborn */
  int damagecount;     /* (the red flash: how much she was just hurt) */
  int allmap;          /* has the computer area map (vanilla: a second can't be picked up) */
  int message_tic;     /* when the game last gave her a message (-1: never) */
  char message[96];    /* ...and what it said */
  int armortype;       /* 0 none, 1 green, 2 blue */
  int backpack;
} evil_player;

extern int evil_fixed_seed;      /* (SDL/i_system.c, patched) */
extern unsigned long evil_seed_value;
extern const char *evil_message;  /* (dsda/messenger.c, patched) */
extern int evil_message_tic;

typedef struct {
  int gametic;
  int gamestate;   /* 0 level, 1 intermission, ... */
  int gameaction;
  int exited;      /* the level's been left (the exit used): stays set till a new game */
  int secret;      /* ...by the secret exit */
  int items, total_items, kills, total_kills, secrets, total_secrets;  /* the tally */
} evil_status;

static int exited, secret_exit;

static double fx(fixed_t v) { return v / 65536.0; }
static double degrees(angle_t a) { return a * (360.0 / 4294967296.0); }

/* argv as for dsda-doom.exe (-iwad, -skill, -warp, -nodraw, ...). Returns 0 when ready. */
EVIL_API int evil_init(int argc, char **argv)
{
  dsda_ParseCommandLineArgs(argc, argv);
  M_LoadDefaults();
  I_PreInitGraphics();
  D_DoomMainSetupForEvil();
  singletics = true;
  /* (no screen melt between levels: it runs on the clock, and while it ran some of her
   * commands went nowhere, a different number each time; and nothing interpolated between
   * tics, which runs on the clock too) */
  dsda_UpdateIntConfig(dsda_config_render_wipescreen, 0, false);
  dsda_UpdateIntConfig(dsda_config_uncapped_framerate, 0, false);
  if (!nodrawers && sdl_window)
  {
    /* (its frames are shown by whoever's watching: evil_draw; and the mouse is theirs: the
     * game took it for mouse look, and the watcher's window had no cursor) */
    SDL_HideWindow(sdl_window);
    SDL_SetWindowGrab(sdl_window, SDL_FALSE);
    SDL_SetRelativeMouseMode(SDL_FALSE);
    SDL_ShowCursor(SDL_ENABLE);
  }
  return 0;
}

/* A new game: skill 1-5, the map (episode 1-4 and map 1-9; Doom II: episode 1, map 1-32),
 * monsters or none. Takes effect on the next tic. */
EVIL_API void evil_new_game(int skill, int episode, int map, int no_monsters)
{
  nomonsters = clnomonsters = no_monsters != 0;  /* (a new game resets the first from the second) */
  exited = secret_exit = 0;
  if (evil_fixed_seed)
    rngseed = 0;  /* (the game adds the seed to what it had: from nothing, the same each time) */
  evil_message = NULL;
  evil_message_tic = -1;
  G_DeferedInitNew(skill - 1, episode, map);
}

/* The random numbers: from the clock, as the game has them (fixed 0: a real run), or from
 * `value` (fixed 1: the same run every time, given the same commands; for testing: a barrel
 * took a different number of shots each run, and all after went its own way). From the next
 * new game on. */
extern int evil_spawned;  /* (p_mobj.c, patched) */

/* Things numbered from 1 again (a level started in this process as in a new one: an episode
 * watched level after level, played as each level was in a process of its own). */
EVIL_API void evil_reset_ids(void)
{
  evil_spawned = 0;
}

EVIL_API void evil_seed(int fixed, unsigned long value)
{
  evil_fixed_seed = fixed != 0;
  evil_seed_value = value;
}

EVIL_API void evil_god(int on)
{
  if (on)
    players[consoleplayer].cheats |= CF_GODMODE;
  else
    players[consoleplayer].cheats &= ~CF_GODMODE;
}

/* Give the player power `power` (pw_invulnerability 0 .. pw_infrared 5) for `tics` tics (for
 * looking round in drive: the light amplification visor shows a dark room as it is). */
EVIL_API void evil_power(int power, int tics)
{
  if (power >= 0 && power < NUMPOWERS)
    players[consoleplayer].powers[power] = tics;
}

/* Put the player at (x, y) facing `angle` (degrees), standing still on the floor there (as a
 * "warp": for checking jumps in the game itself). Returns 0 if something's in the way. */
EVIL_API int evil_warp(double x, double y, double angle)
{
  mobj_t *mo = players[consoleplayer].mo;
  dboolean moved;
  if (!mo)
    return 0;
  P_MapStart();  /* (the game brackets its moves so, and checks it does) */
  moved = P_TeleportMove(mo, (fixed_t)(x * 65536.0), (fixed_t)(y * 65536.0), false);
  P_MapEnd();
  if (!moved)
    return 0;
  mo->z = mo->floorz;
  mo->momx = mo->momy = mo->momz = 0;
  mo->angle = (angle_t)(long long)(angle / 360.0 * 4294967296.0);
  players[consoleplayer].viewz = mo->z + players[consoleplayer].viewheight;
  return 1;
}

/* One tic, with this command for the player (forwardmove/sidemove as in a ticcmd: a run is
 * 50 forward, 40 sideways; turn: angleturn, <<16 for the angle, positive to the left;
 * buttons: BT_ATTACK 1, BT_USE 2, BT_CHANGE 4 with the weapon in bits 3-5). */
EVIL_API void evil_tic(int forward, int side, int turn, int buttons)
{
  ticcmd_t *cmd;
  /* (no I_StartTic: her commands are set here, not read from a keyboard; and the window's
   * events, drawing for a watcher, paused the game when it wasn't in focus) */
  dsda_ResetPauseMode();
  menuactive = false;
  /* (the slot the game reads this tic, and no other: drawing a frame calls NetUpdate partway
   * through, which builds commands for the clock's tics and moves maketic on, and a command
   * written to maketic's slot then went nowhere: a tic of hers lost now and then) */
  maketic = gametic;
  cmd = &local_cmds[consoleplayer][gametic % BACKUPTICS];
  /* (not G_BuildTiccmd: it folds in the mouse and the smoothed view's turning, which with
   * frames being drawn turned her a little each tic of its own accord) */
  memset(cmd, 0, sizeof *cmd);
  cmd->forwardmove = (signed char)forward;
  cmd->sidemove = (signed char)side;
  cmd->angleturn = (signed short)turn;
  /* (the slot's weapon, as G_BuildTiccmd picks it outside demo compatibility: slot 3 the super
   * shotgun once she has it; without, a slot-3 press asked for a shotgun she hadn't) */
  if ((buttons & BT_CHANGE) && gamemode == commercial &&
      ((buttons & BT_WEAPONMASK) >> BT_WEAPONSHIFT) == wp_shotgun &&
      players[consoleplayer].weaponowned[wp_supershotgun])
    buttons = (buttons & ~BT_WEAPONMASK) | (wp_supershotgun << BT_WEAPONSHIFT);
  cmd->buttons = (byte)buttons;
  M_Ticker();
  G_Ticker();
  if (gameaction == ga_completed || gameaction == ga_victory || gamestate == GS_INTERMISSION)
  {
    if (!exited)
      secret_exit = secretexit;
    exited = 1;
  }
  gametic++;
  maketic = gametic;
}

/* Draw the frame as the game would (the player's view, the status bar) and copy it out as
 * palette indices, a row at a time (out: width * height bytes). Returns width << 16 | height,
 * or 0 if there's no room or nothing's drawn (-nodraw). For watching a run. */
extern dboolean setsizeneeded;
void R_ExecuteSetViewSize(void);

EVIL_API int evil_draw(unsigned char *out, int max)
{
  int y, w, h;
  if (nodrawers)
    return 0;
  if (gamestate != GS_LEVEL)
    return 0;
  /* (the view and the status bar, the tic as it is: no menus, wipes or presenting) */
  if (setsizeneeded)
    R_ExecuteSetViewSize();  /* (the view's size and buffers: D_Display does this first) */
  R_InterpolateView(&players[displayplayer], FRACUNIT);
  R_RenderPlayerView(&players[displayplayer]);
  R_RestoreInterpolations();
  ST_Drawer(true);
  w = screens[0].width;
  h = screens[0].height;
  if (!screens[0].data || w * h > max)
    return 0;
  for (y = 0; y < h; y++)
    memcpy(out + y * w, screens[0].data + y * screens[0].pitch, w);
  return (w << 16) | h;
}

EVIL_API void evil_get_status(evil_status *out)
{
  out->gametic = gametic;
  out->gamestate = gamestate;
  out->gameaction = gameaction;
  out->exited = exited;
  out->secret = secret_exit;
  out->items = players[consoleplayer].itemcount;
  out->total_items = totalitems;
  out->kills = players[consoleplayer].killcount;
  out->total_kills = totalkills;
  out->secrets = players[consoleplayer].secretcount;
  out->total_secrets = totalsecret;
}

/* Every sector's floor and ceiling now: out[2i], out[2i+1]. Returns how many sectors. */
EVIL_API int evil_sectors(double *out, int max)
{
  int i, n = numsectors < max ? numsectors : max;
  for (i = 0; i < n; i++)
  {
    out[2 * i] = fx(sectors[i].floorheight);
    out[2 * i + 1] = fx(sectors[i].ceilingheight);
  }
  return numsectors;
}

/* How far into its missile attack mo is: the tics from the attack's first frame to now, along
 * the frames it goes through (-1: it isn't in them). */
static int attack_tics(const mobj_t *mo)
{
  statenum_t first = mo->info->missilestate;
  statenum_t at = first;
  int tics = 0, i;
  if (first == S_NULL || !mo->state)
    return -1;
  for (i = 0; i < 16 && at != S_NULL; i++)
  {
    if (&states[at] == mo->state)
      return tics + (states[at].tics - mo->tics);
    if (states[at].tics < 0)
      return -1;
    tics += states[at].tics;
    at = states[at].nextstate;
    if (at == first || at == mo->info->seestate || at == mo->info->spawnstate)
      return -1;
  }
  return -1;
}

/* The things about (monsters, missiles, items, decorations, corpses), up to max. Returns
 * how many there are in all. */
EVIL_API int evil_mobjs(evil_mobj *out, int max)
{
  thinker_t *th;
  int n = 0;
  if (gamestate != GS_LEVEL)
    return 0;
  for (th = thinkercap.next; th != &thinkercap; th = th->next)
  {
    mobj_t *mo;
    if (th->function != P_MobjThinker)
      continue;
    mo = (mobj_t *)th;
    if (n < max)
    {
      evil_mobj *o = &out[n];
      o->id = mo->evil_id;
      o->type = mo->type;
      o->x = fx(mo->x); o->y = fx(mo->y); o->z = fx(mo->z);
      o->momx = fx(mo->momx); o->momy = fx(mo->momy); o->momz = fx(mo->momz);
      o->angle = degrees(mo->angle);
      o->radius = fx(mo->radius); o->height = fx(mo->height);
      o->health = mo->health;
      o->flags = mo->flags;
      o->owner_type = (mo->flags & MF_MISSILE) && mo->target ? (int)mo->target->type : -1;
      o->state = mo->state ? (int)(mo->state - states) : -1;
      o->after_her = mo->target && mo->target->player ? 1 : 0;
      o->attack = (mo->flags & MF_COUNTKILL) || mo->type == MT_SKULL ? attack_tics(mo) : -1;
    }
    n++;
  }
  return n;
}

EVIL_API void evil_player_state(evil_player *out)
{
  player_t *p = &players[consoleplayer];
  int i;
  memset(out, 0, sizeof *out);
  out->playerstate = p->playerstate;
  out->health = p->health;
  out->armor = p->armorpoints[0];
  out->readyweapon = p->readyweapon;
  out->pendingweapon = p->pendingweapon;
  for (i = 0; i < 9 && i < NUMWEAPONS; i++)
    out->weaponowned[i] = p->weaponowned[i];
  for (i = 0; i < 4 && i < NUMAMMO; i++)
    out->ammo[i] = p->ammo[i];
  for (i = 0; i < 6 && i < NUMCARDS; i++)
    out->cards[i] = p->cards[i];
  out->ironfeet = p->powers[pw_ironfeet];
  out->damagecount = p->damagecount;
  out->allmap = p->powers[pw_allmap] != 0;
  out->message_tic = evil_message_tic;
  out->armortype = p->armortype;
  out->backpack = p->backpack ? 1 : 0;
  if (evil_message)
    snprintf(out->message, sizeof out->message, "%s", evil_message);
  if (p->mo)
  {
    out->x = fx(p->mo->x); out->y = fx(p->mo->y); out->z = fx(p->mo->z);
    out->angle = degrees(p->mo->angle);
    out->momx = fx(p->mo->momx); out->momy = fx(p->mo->momy); out->momz = fx(p->mo->momz);
  }
}

/* Spawn a thing of type `type` (mobjtype_t) on the floor at (x, y), facing `angle` (degrees);
 * `ambush`: deaf (waits to see her). Returns its evil_id, or -1. For arenas: the fight set up
 * where wanted, the level's own monsters left out (-nomonsters). */
EVIL_API int evil_spawn(int type, double x, double y, double angle, int ambush)
{
  mobj_t *mo;
  dboolean fits;
  if (gamestate != GS_LEVEL || type < 0 || type >= num_mobj_types)
    return -1;
  mo = P_SpawnMobj((fixed_t)(x * 65536.0), (fixed_t)(y * 65536.0), ONFLOORZ, type);
  if (!mo)
    return -1;
  P_MapStart();
  fits = P_CheckPosition(mo, mo->x, mo->y);
  P_MapEnd();
  if (!fits)
  {
    P_RemoveMobj(mo);  /* (in a wall, or on another) */
    return -1;
  }
  mo->angle = (angle_t)(long long)(angle / 360.0 * 4294967296.0);
  if (ambush)
    mo->flags |= MF_AMBUSH;
  if (mo->flags & MF_COUNTKILL)
    totalkills++;
  return mo->evil_id;
}

/* Take every pickup off the level (an arena's fight with only what she's given). Returns how many
 * went. */
EVIL_API int evil_clear_items(void)
{
  thinker_t *th, *next;
  int n = 0;
  if (gamestate != GS_LEVEL)
    return 0;
  for (th = thinkercap.next; th != &thinkercap; th = next)
  {
    next = th->next;
    if (th->function == P_MobjThinker && (((mobj_t *)th)->flags & MF_SPECIAL))
    {
      P_RemoveMobj((mobj_t *)th);
      n++;
    }
  }
  return n;
}

/* Give her a loadout: weapons by weapontype_t bit (fist 0, pistol 1, shotgun 2, chaingun 3,
 * missile 4, plasma 5, bfg 6, chainsaw 7, supershotgun 8), ammo (clip, shell, cell, missile),
 * the backpack, health and armor (armortype 1 green, 2 blue). For arenas. */
EVIL_API void evil_give(int weapons, int clip, int shell, int cell, int missile, int backpack, int health, int armor, int armortype)
{
  player_t *p = &players[consoleplayer];
  int i;
  for (i = 0; i < NUMWEAPONS && i < 9; i++)
    p->weaponowned[i] = (weapons >> i) & 1;
  if (backpack && !p->backpack)
  {
    p->backpack = true;
    for (i = 0; i < NUMAMMO; i++)
      p->maxammo[i] *= 2;
  }
  p->ammo[am_clip] = clip < p->maxammo[am_clip] ? clip : p->maxammo[am_clip];
  p->ammo[am_shell] = shell < p->maxammo[am_shell] ? shell : p->maxammo[am_shell];
  p->ammo[am_cell] = cell < p->maxammo[am_cell] ? cell : p->maxammo[am_cell];
  p->ammo[am_misl] = missile < p->maxammo[am_misl] ? missile : p->maxammo[am_misl];
  p->health = health;
  if (p->mo)
    p->mo->health = health;
  p->armorpoints[0] = armor;
  p->armortype = armortype;
}
