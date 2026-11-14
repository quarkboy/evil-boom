/* SPDX-License-Identifier: GPL-3.0-or-later
 * Copyright (C) 2026 Quarkboy
 *
 * This program is free software; you can redistribute it and/or modify it under the terms of
 * the GNU General Public License as published by the Free Software Foundation; either version 3
 * of the License, or (at your option) any later version. It is built into DSDA-Doom
 * (GPL-2.0-or-later, used here under version 3) and uses its sources. See LICENSE.
 *
 * This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
 * without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See
 * the GNU General Public License for more details. */

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
#include "p_inter.h"
#include "p_pspr.h"
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
#include "s_sound.h"
#include "i_sound.h"
#include "f_finale.h"
#include "f_wipe.h"
#include "wi_stuff.h"
#include "hu_stuff.h"
#include "dsda/global.h"
#include "dsda/mapinfo.h"
#include "dsda/utility.h"
#include "sounds.h"
#include <limits.h>

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
  int strength;        /* berserk this level (pw_strength: a punch does ten times the damage) */
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

/* Presentation (the stream console's, evil_present): Doom's own screens drawn and run as the
 * game runs them, the title card, the tally, the finale, the menu and the melt between them.
 * Off, everything is as it was: only the level drawn, the menu shut every tic, no melt. */
static int evil_presenting;
/* a melt under way (stepped by evil_wipe_tic, the game held meanwhile, as Doom holds it) */
static int evil_wipe_on;
/* the game's move on to the next level held (gameaction ga_worlddone, after the tally or a
 * finale): the caller starts the next level as it starts any, so it plays as it would */
static int evil_held;
/* a frame's been drawn: there's something to melt from */
static int evil_drawn_once;

static void evil_render(fixed_t frac);
static int evil_wipe_begin(void);
static void evil_wipe_step(int ticks);
void D_EvilPageDrawer(void);  /* (d_main.c, patched) */
int WI_EvilWaiting(void);     /* (wi_stuff.c, patched) */
extern gamestate_t wipegamestate;
extern menu_t *currentMenu;   /* (m_menu.c) */
extern short itemOn;
extern menu_t SkillDef;
extern finalestage_t finalestage;  /* (f_finale.c) */
extern int finalecount;
extern const char *finaletext;

static double fx(fixed_t v) { return v / 65536.0; }
static double degrees(angle_t a) { return a * (360.0 / 4294967296.0); }

/* argv as for dsda-doom.exe (-iwad, -skill, -warp, -nodraw, ...), and two of ours (taken out
 * before the game sees the arguments): -evilsoundcap, the sound mixed for the caller
 * (evil_grab_sound) instead of played, as DSDA's video dump has it; -evilsoundfont FILE, the
 * music played by FluidSynth with that SoundFont (else by the OPL synth, a Sound Blaster's).
 * Returns 0 when ready. */
EVIL_API int evil_init(int argc, char **argv)
{
  int i, kept = 0;
  const char *soundfont = NULL;
  for (i = 0; i < argc; i++)
  {
    if (!strcmp(argv[i], "-evilsoundcap"))
      I_SetSoundCap();
    else if (!strcmp(argv[i], "-evilsoundfont") && i + 1 < argc)
      soundfont = argv[++i];
    else
      argv[kept++] = argv[i];
  }
  argc = kept;
  dsda_ParseCommandLineArgs(argc, argv);
  M_LoadDefaults();
  /* (the music's player said each time, not left to the game's saved settings: with none
   * asked for, OPL as before FluidSynth was built in. A SoundFont that won't load falls back to
   * OPL too. FluidSynth's reverb and chorus on: the songs ask for them, as the Sound Canvas
   * played them) */
  /* (set without the change's callback, which stops and restarts the music: the sound isn't
   * up yet, and that crashed. The menu's setting up reads it, as from the saved settings) */
  dsda_HackStringConfig(dsda_config_snd_midiplayer, soundfont ? "fluidsynth" : "opl", false);
  if (soundfont)
  {
    dsda_HackStringConfig(dsda_config_snd_soundfont, soundfont, false);
    dsda_UpdateIntConfig(dsda_config_mus_fluidsynth_reverb, 1, false);
    dsda_UpdateIntConfig(dsda_config_mus_fluidsynth_chorus, 1, false);
  }
  I_PreInitGraphics();
  D_DoomMainSetupForEvil();
  singletics = true;
  /* (no screen melt between levels: it runs on the clock, and while it ran some of her
   * commands went nowhere, a different number each time; and nothing interpolated between
   * tics by the clock, which the game's own loop does) */
  dsda_UpdateIntConfig(dsda_config_render_wipescreen, 0, false);
  dsda_UpdateIntConfig(dsda_config_uncapped_framerate, 0, false);
  /* (drawn: a frame between two tics drawn as things are between them, as far between as the
   * caller says (evil_draw_at), for a window at the screen's own rate. Only the drawing
   * changes: the game keeps where things were a tic back, and the heights of moving floors are
   * set between for the drawing and put back after it, before anything reads them) */
  movement_smooth = !nodrawers;
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
  /* (a new game sets the first from the second, then, as the skill's modifiers are worked out,
   * from the setting, or from the command line's flag while a demo's on: all four, or the level
   * starts as the game was started) */
  nomonsters = clnomonsters = no_monsters != 0;
  dsda_UpdateFlag(dsda_arg_nomonsters, no_monsters != 0);
  dsda_UpdateIntConfig(dsda_config_no_monsters, no_monsters != 0, false);
  exited = secret_exit = 0;
  if (evil_fixed_seed)
    rngseed = 0;  /* (the game adds the seed to what it had: from nothing, the same each time) */
  evil_message = NULL;
  evil_message_tic = -1;
  evil_held = 0;
  G_DeferedInitNew(skill - 1, episode, map);
}

/* Presentation on or off (the stream console's game): see evil_presenting. With it on, a
 * melt holds the game: the caller steps it (evil_wipe_tic) instead of the game while
 * evil_screen says one's under way. */
EVIL_API void evil_present(int on)
{
  evil_presenting = on != 0;
  dsda_UpdateIntConfig(dsda_config_render_wipescreen, evil_presenting, false);
  /* (Doom's tally as Doom has it: not DSDA's speedrunning clock in its corner) */
  if (evil_presenting)
    dsda_UpdateIntConfig(dsda_config_show_level_splits, 0, false);
  if (!evil_presenting)
  {
    evil_wipe_on = 0;
    evil_held = 0;
    menuactive = false;
  }
}

/* The title card, held there (no demos after it: a stream shows the card), with its music. What's on
 * before it (a level) is left as it is underneath; a new game starts from it. */
EVIL_API void evil_title(void)
{
  if (menuactive)
    M_ClearMenus();
  gameaction = ga_nothing;
  players[consoleplayer].playerstate = PST_LIVE;
  dsda_ResetPauseMode();
  advancedemo = false;
  gamestate = GS_DEMOSCREEN;
  D_SetPage("TITLEPIC", INT_MAX / 2, gamemode == commercial ? mus_dm2ttl : mus_intro);
  exited = secret_exit = 0;
  evil_held = 0;
}

/* The level left by its exit (`secret`: the secret one), as a switch or a line leaves it: the
 * tally and what follows as the game has them (the stream console's: a level skipped). Only in
 * a level not already left. */
EVIL_API void evil_exit(int secret)
{
  if (gamestate != GS_LEVEL || exited || gameaction != ga_nothing)
    return;
  if (secret)
    G_SecretExitLevel(0);
  else
    G_ExitLevel(0);
}

/* The level's title as the automap has it ("level 7: dead simple", "E1M1: Hangar"), into out
 * (max bytes, ended); its length. Empty outside a level. */
EVIL_API int evil_level_title(char *out, int max)
{
  dsda_string_t title;
  if (max <= 0)
    return 0;
  out[0] = 0;
  dsda_HUTitle(&title);
  if (title.string)
  {
    strncpy(out, title.string, (size_t)max - 1);
    out[max - 1] = 0;
  }
  dsda_FreeString(&title);
  return (int)strlen(out);
}

/* The finale's text while a finale shows it (Doom's episode endings, Doom II's stories between
 * levels), as much of it as is on screen yet (it's typed out, a character every TEXTSPEED tics
 * from its tenth, as F_TextWrite has it), into out (max bytes, ended); its length. Empty
 * otherwise. */
EVIL_API int evil_finale_text(char *out, int max)
{
  int shown;
  if (max <= 0)
    return 0;
  out[0] = 0;
  if (gamestate == GS_FINALE && finaletext)
  {
    shown = (finalecount - 10) / 3 /* (TEXTSPEED) */;
    if (shown < 0)
      shown = 0;
    if (shown > max - 1)
      shown = max - 1;
    strncpy(out, finaletext, (size_t)shown);
    out[shown] = 0;
  }
  return (int)strlen(out);
}

/* A tic of the melt, the game held (as Doom holds it); one begun if it's due. */
EVIL_API void evil_wipe_tic(void)
{
  if (evil_wipe_begin())
    evil_wipe_step(1);
}

/* The menu driven as a player drives it, for the stream to see: 0 open it (the main menu),
 * 1 the skull on item `arg`, 2 choose the item it's on, 3 shut it; each with the menu's own
 * sound. Choosing on the skill menu only clicks: the caller starts the game (evil_new_game), as
 * it starts any game, so it plays as one started any other way. */
EVIL_API void evil_menu(int action, int arg)
{
  menuitem_t *item;
  switch (action)
  {
    case 0:
      M_StartControlPanel();
      S_StartOptionalSound(g_sfx_mnuopn, g_sfx_swtchn, true);
      break;
    case 1:
      if (menuactive && currentMenu && arg >= 0 && arg < currentMenu->numitems)
      {
        itemOn = arg;
        S_StartOptionalSound(g_sfx_mnumov, g_sfx_menu, true);
      }
      break;
    case 2:
      if (!menuactive || !currentMenu)
        break;
      item = &currentMenu->menuitems[itemOn];
      if (currentMenu != &SkillDef && item->routine && item->status)
      {
        currentMenu->lastOn = itemOn;
        item->routine(itemOn);
      }
      S_StartOptionalSound(g_sfx_mnuact, g_sfx_pistol, true);
      break;
    default:
      M_ClearMenus();
      break;
  }
}

/* The screen as it stands: out[0] the game state (0 a level, 1 the tally, 2 a finale, 3 the
 * title), out[1] a melt under way or due, out[2] the menu up, out[3] the move on to the next
 * level held (the caller's to start), out[4] the screen waits for a press (the tally counted;
 * Doom II's finale text all shown: a press finishes it, a second goes on; Doom's goes on by
 * itself), out[5] the menu's current item. */
EVIL_API void evil_screen(int *out)
{
  out[0] = gamestate;
  out[1] = evil_wipe_on || (evil_presenting && evil_drawn_once && gamestate != wipegamestate);
  out[2] = menuactive != 0;
  out[3] = evil_held;
  out[4] = (gamestate == GS_INTERMISSION && WI_EvilWaiting()) ||
           (gamestate == GS_FINALE && gamemode == commercial && finalestage == FINALE_STAGE_TEXT &&
            finaletext && finalecount > (int)strlen(finaletext) * 3 /* (TEXTSPEED) */);
  out[5] = itemOn;
}

/* The sounds the game's started since the last call, as she'd hear them (up to max; the oldest
 * of a long run of them lost): ten numbers each, as evil_sound_t has them (tic, sfx, origin id,
 * origin type, own, volume, separation, x, y, chasing). Returns how many. */
static int evil_sounds_read;

EVIL_API int evil_sounds_since(int *out, int max)
{
  int n = 0;
  if (evil_sound_count - evil_sounds_read > EVIL_SOUNDS)
    evil_sounds_read = evil_sound_count - EVIL_SOUNDS;
  if (evil_sounds_read > evil_sound_count)
    evil_sounds_read = evil_sound_count;
  while (evil_sounds_read < evil_sound_count && n < max)
  {
    evil_sound_t *e = &evil_sounds[evil_sounds_read % EVIL_SOUNDS];
    int *o = out + 10 * n;
    o[0] = e->tic; o[1] = e->sfx; o[2] = e->origin_id; o[3] = e->origin_type; o[4] = e->own;
    o[5] = e->volume; o[6] = e->separation; o[7] = e->x; o[8] = e->y; o[9] = e->chasing;
    evil_sounds_read++;
    n++;
  }
  return n;
}

/* A sound's name by its number ("pistol", "firsht", "podth1", ...), into out (max bytes, ended
 * with a 0). Returns 0 past the last sound. */
EVIL_API int evil_sfx_name(int sfx, char *out, int max)
{
  const char *name;
  if (sfx < 1 || sfx >= num_sfx || max < 1)
    return 0;
  name = S_sfx[sfx].name ? S_sfx[sfx].name : "";
  snprintf(out, max, "%s", name);
  return 1;
}

/* The sounds a kind of thing makes, by number (0: none): when it sees her (a missile: as it's
 * fired), attacking, in pain, dying (a missile: as it goes off), wandering about. The game picks
 * among a few of some at random (the three "podth", the two "bgsit"): the first is given.
 * Returns 0 for no such type. */
EVIL_API int evil_mobj_sounds(int type, int *out)
{
  const mobjinfo_t *info;
  if (type < 0 || type >= num_mobj_types)
    return 0;
  info = &mobjinfo[type];
  out[0] = info->seesound; out[1] = info->attacksound; out[2] = info->painsound;
  out[3] = info->deathsound; out[4] = info->activesound;
  return 1;
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

/* The hits on her since the last call (up to max; the oldest of a long run of them lost):
 * eight numbers each, as evil_hit_t has them (health, armor, inflictor, source, self, cause,
 * side, source id).
 * Returns how many. */
static int evil_hits_read;

EVIL_API int evil_hits_since(int *out, int max)
{
  int n = 0;
  if (evil_hit_count - evil_hits_read > EVIL_HITS)
    evil_hits_read = evil_hit_count - EVIL_HITS;
  if (evil_hits_read > evil_hit_count)
    evil_hits_read = evil_hit_count;
  while (evil_hits_read < evil_hit_count && n < max)
  {
    evil_hit_t *h = &evil_hits[evil_hits_read % EVIL_HITS];
    int *o = out + 8 * n;
    o[0] = h->health; o[1] = h->armor; o[2] = h->inflictor; o[3] = h->source;
    o[4] = h->self; o[5] = h->cause; o[6] = h->side; o[7] = h->source_id;
    evil_hits_read++;
    n++;
  }
  return n;
}

extern int evil_pellets[7];  /* (p_map.c, patched) */

/* Her bullets and pellets so far: fired, those that hit a thing that can be shot, those of
 * them a barrel, those that hit nothing after one of the same tic had killed; and her shots
 * (a tic's pellets), and those of them with no pellet on a thing; and the pellets fired with
 * the trigger held since the shot before (seven numbers). */
EVIL_API void evil_pellets_so_far(int *out)
{
  int i;
  for (i = 0; i < 7; i++)
    out[i] = evil_pellets[i];
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
 * buttons: BT_ATTACK 1, BT_USE 2, BT_CHANGE 4 with the weapon in bits 3-6; 0x100 a weapon press
 * for the very gun its number says). */
EVIL_API void evil_tic(int forward, int side, int turn, int buttons)
{
  ticcmd_t *cmd;
  /* (no I_StartTic: her commands are set here, not read from a keyboard; and the window's
   * events, drawing for a watcher, paused the game when it wasn't in focus) */
  dsda_ResetPauseMode();
  if (!evil_presenting)
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
  /* (a press as G_BuildTiccmd picks a slot's weapon outside demo compatibility: slot 3 the
   * super shotgun once she has it, slot 1 the chainsaw but the fist while she's berserk (a punch
   * then outdoing the saw); without, a slot-3 press asked for a shotgun she hadn't. An exact
   * press, 0x100, is for the very gun its number says (the plain shotgun 2, the super 8, the fist
   * 0, the chainsaw 7): as it is) */
  if (!(buttons & 0x100) && (buttons & BT_CHANGE))
  {
    if (gamemode == commercial &&
        ((buttons & BT_WEAPONMASK) >> BT_WEAPONSHIFT) == wp_shotgun &&
        players[consoleplayer].weaponowned[wp_supershotgun])
      buttons = (buttons & ~BT_WEAPONMASK) | (wp_supershotgun << BT_WEAPONSHIFT);
    if (((buttons & BT_WEAPONMASK) >> BT_WEAPONSHIFT) == wp_fist &&
        players[consoleplayer].weaponowned[wp_chainsaw] && !players[consoleplayer].powers[pw_strength])
      buttons = (buttons & ~BT_WEAPONMASK) | (wp_chainsaw << BT_WEAPONSHIFT);
  }
  cmd->buttons = (byte)buttons;
  /* (presenting: the move on to the next level held for the caller, the screen as it is) */
  if (evil_presenting && gameaction == ga_worlddone)
    evil_held = 1;
  M_Ticker();
  if (!evil_held)
    G_Ticker();
  if (gameaction == ga_completed || gameaction == ga_victory || gamestate == GS_INTERMISSION)
  {
    if (!exited)
      secret_exit = secretexit;
    exited = 1;
  }
  /* (sounds placed for where she is now, finished ones let go: the game's own loop does it a
   * frame; nothing without sound, -nosound) */
  S_UpdateSounds();
  gametic++;
  maketic = gametic;
}

/* Sound on or off (a watcher's: off while paused or not at the game's own speed, where it'd
 * lag or pile up). Off stops what's playing too. Never the game's mute when its own window,
 * which no one sees, isn't focused. No effect without sound (-nosound -nomusic). */
EVIL_API void evil_sound(int on)
{
  dsda_UpdateIntConfig(dsda_config_mute_unfocused_window, 0, false);
  dsda_UpdateIntConfig(dsda_config_mute_sfx, !on, false);
  dsda_UpdateIntConfig(dsda_config_mute_music, !on, false);
  S_ResetVolume();
  if (!on)
    S_Stop();
}

/* The sound's loudness, as the stream console sets it: sfx and music as the game's own settings
 * have them (0-15, its Sound menu's), master over the whole mix (0-100); -1 leaves one as it is.
 * Not saved to the game's config file. */
extern int evil_master_volume;

EVIL_API void evil_volume(int sfx, int music, int master)
{
  if (sfx >= 0)
    dsda_UpdateIntConfig(dsda_config_sfx_volume, sfx > 15 ? 15 : sfx, false);
  if (music >= 0)
    dsda_UpdateIntConfig(dsda_config_music_volume, music > 15 ? 15 : music, false);
  if (master >= 0)
    evil_master_volume = master > 100 ? 100 : master;
}

/* The loudness now: out[0] sfx, out[1] music (0-15), out[2] master (0-100). */
EVIL_API void evil_get_volume(int *out)
{
  out[0] = dsda_IntConfig(dsda_config_sfx_volume);
  out[1] = dsda_IntConfig(dsda_config_music_volume);
  out[2] = evil_master_volume;
}

/* The game's sound for the next `samples` sample frames (16-bit stereo, interleaved: out holds
 * 2 * samples), mixed now, as DSDA's video dump takes it a frame at a time; only with
 * -evilsoundcap. Returns the sample rate, or 0 if there's none to take. A recording's: a tic's
 * worth after each tic. */
EVIL_API int evil_grab_sound(short *out, int samples)
{
  extern int snd_samplerate;
  unsigned char *got = I_GrabSound(samples);
  if (!got)
    return 0;
  memcpy(out, got, (size_t)samples * 4);
  return snd_samplerate;
}

/* The sound's sample rate (what evil_grab_sound's samples are at). */
EVIL_API int evil_sound_rate(void)
{
  extern int snd_samplerate;
  return snd_samplerate;
}

extern dboolean setsizeneeded;
void R_ExecuteSetViewSize(void);

/* The screen as it is into screens[0], as D_Display draws it (no melt): a level's view and
 * status bar (presenting, its messages too), or the tally, the finale, the title card; the menu
 * over any of them while it's up (presenting). Not presenting, a level's only. */
static void evil_render(fixed_t frac)
{
  /* (as D_Display has it: the frame's the first since a tic was played or not, which the
   * weapon's own smoothing goes by. Without, it drew the weapon from where it was when it last
   * changed, and between tics the gun jumped about) */
  if (!I_StartDisplay())
    return;
  if (setsizeneeded)
    R_ExecuteSetViewSize();  /* (the view's size and buffers: D_Display does this first) */
  if (gamestate == GS_LEVEL)
  {
    R_InterpolateView(&players[displayplayer], frac);
    R_RenderPlayerView(&players[displayplayer]);
    R_RestoreInterpolations();
    ST_Drawer(true);
    if (evil_presenting)
      HU_Drawer();
  }
  else
  {
    V_BeginUIDraw();
    if (gamestate == GS_INTERMISSION)
      WI_Drawer();
    else if (gamestate == GS_FINALE)
      F_Drawer();
    else if (gamestate == GS_DEMOSCREEN)
      D_EvilPageDrawer();
    V_EndUIDraw();
  }
  if (evil_presenting && menuactive)
  {
    V_BeginMenuDraw();
    if (M_MenuIsShaded())
      M_ShadedScreen(0);
    V_EndMenuDraw();
    M_Drawer();
  }
  I_EndDisplay();
}

/* A step of the melt. The play's random numbers kept as they were: the melt takes some (its
 * columns' starts), and at a level's start they'd change how it plays. */
static void evil_wipe_step(int ticks)
{
  rng_t kept = rng;
  if (wipe_ScreenWipe(ticks))
    evil_wipe_on = 0;
  rng = kept;
}

/* The melt begun if the screen's changed since the last frame drawn (a level begun or begun
 * again, the tally, a finale, the title), as D_Display begins it: the last frame melting into
 * the new one. Whether one's under way. */
static int evil_wipe_begin(void)
{
  if (!evil_presenting || nodrawers || evil_wipe_on)
    return evil_wipe_on;
  if (gamestate == wipegamestate)
    return 0;
  if (!evil_drawn_once || !screens[0].data)
  {
    wipegamestate = gamestate;  /* (nothing drawn yet to melt from) */
    return 0;
  }
  wipe_StartScreen();  /* (screens[0]: the last frame drawn) */
  evil_render(FRACUNIT);
  wipe_EndScreen();    /* (and screens[0] back to the last frame, till the first step) */
  wipegamestate = gamestate;
  evil_wipe_on = 1;
  return 1;
}

/* Draw the frame as the game would (the player's view, the status bar) and copy it out as
 * palette indices, a row at a time (out: width * height bytes): `between` (0 to 1) how far from
 * the tic before to this one, 1 the tic as it is. Returns width << 16 | height, or 0 if there's
 * no room or nothing's drawn (-nodraw). For watching a run. */
EVIL_API int evil_draw_at(unsigned char *out, int max, double between)
{
  int y, w, h;
  fixed_t frac = between <= 0 ? 0 : between >= 1 ? FRACUNIT : (fixed_t)(between * FRACUNIT);
  if (nodrawers)
    return 0;
  if (!evil_presenting)
  {
    /* (the view and the status bar: no menus, wipes or other screens) */
    if (gamestate != GS_LEVEL)
      return 0;
    evil_render(frac);
  }
  else if (!evil_wipe_begin())
    evil_render(frac);
  w = screens[0].width;
  h = screens[0].height;
  if (!screens[0].data || w * h > max)
    return 0;
  for (y = 0; y < h; y++)
    memcpy(out + y * w, screens[0].data + y * screens[0].pitch, w);
  evil_drawn_once = 1;
  return (w << 16) | h;
}

/* evil_draw_at the tic as it is. */
EVIL_API int evil_draw(unsigned char *out, int max)
{
  return evil_draw_at(out, max, 1);
}

/* The watcher's window (evil-boom watch): the picture the caller paints, shown at the screen's
 * own rate (vsync where there is one), scaled to fit. Its keys, wheel and closing are the
 * caller's, kept from the game: drawing a frame polls the events (NetUpdate), and the game would
 * take them for its own (keys to its menu, Alt-F4 quitting it outright). */
static SDL_Window *evil_window;
static SDL_Renderer *evil_renderer;
static SDL_Texture *evil_texture;
static int evil_texture_w, evil_texture_h;
static Uint32 evil_window_id;
#define EVIL_EVENTS 64
static int evil_events[EVIL_EVENTS * 2];
static int evil_event_count;

static void evil_event(int kind, int value)
{
  if (evil_event_count < EVIL_EVENTS)
  {
    evil_events[evil_event_count * 2] = kind;
    evil_events[evil_event_count * 2 + 1] = value;
    evil_event_count++;
  }
}

/* (each event as it comes in, before anyone polls: the window's kept for the caller (1 a key
 * pressed, its SDL keycode; 2 the wheel, up positive; 3 closed or quit) and taken out) */
static int SDLCALL evil_event_filter(void *data, SDL_Event *e)
{
  Uint32 id = 0;
  (void)data;
  switch (e->type)
  {
    case SDL_QUIT:
      evil_event(3, 0);
      return 0;
    case SDL_KEYDOWN:
    case SDL_KEYUP:
      id = e->key.windowID;
      if (id == evil_window_id && e->type == SDL_KEYDOWN && !e->key.repeat)
        evil_event(1, e->key.keysym.sym);
      break;
    case SDL_TEXTINPUT:
      id = e->text.windowID;
      break;
    case SDL_TEXTEDITING:
      id = e->edit.windowID;
      break;
    case SDL_MOUSEMOTION:
      id = e->motion.windowID;
      break;
    case SDL_MOUSEBUTTONDOWN:
    case SDL_MOUSEBUTTONUP:
      id = e->button.windowID;
      break;
    case SDL_MOUSEWHEEL:
      id = e->wheel.windowID;
      if (id == evil_window_id && e->wheel.y)
        evil_event(2, e->wheel.direction == SDL_MOUSEWHEEL_FLIPPED ? -e->wheel.y : e->wheel.y);
      break;
    case SDL_WINDOWEVENT:
      id = e->window.windowID;
      if (id == evil_window_id && e->window.event == SDL_WINDOWEVENT_CLOSE)
        evil_event(3, 0);
      break;
    default:
      return 1;
  }
  return id != evil_window_id;
}

/* The window opened for a picture `w` by `h`, the window `window_w` by `window_h` (0: the
 * picture's size; it can be resized, the picture scaled to fit). flags: 1 vsync, 2 full screen
 * (the desktop's, borderless). out[0]: 1 if showing a frame waits for the screen's refresh;
 * out[1]: the screen's rate (Hz, 0 unknown). Returns 1 if it's open. */
EVIL_API int evil_window_open(const char *title, int w, int h, int window_w, int window_h,
                              int flags, int *out)
{
  SDL_RendererInfo info;
  SDL_DisplayMode mode;
  Uint32 vsync = (flags & 1) ? SDL_RENDERER_PRESENTVSYNC : 0;
  if (evil_window)
    return 1;
  if (nodrawers || !SDL_WasInit(SDL_INIT_VIDEO))
    return 0;
  SDL_SetHint(SDL_HINT_RENDER_SCALE_QUALITY, "linear");
  evil_window = SDL_CreateWindow(title, SDL_WINDOWPOS_CENTERED, SDL_WINDOWPOS_CENTERED,
                                 window_w > 0 ? window_w : w, window_h > 0 ? window_h : h,
                                 SDL_WINDOW_RESIZABLE |
                                     ((flags & 2) ? SDL_WINDOW_FULLSCREEN_DESKTOP : 0));
  if (!evil_window)
  {
    lprintf(LO_WARN, "evil_window_open: %s\n", SDL_GetError());
    return 0;
  }
  evil_renderer = SDL_CreateRenderer(evil_window, -1, SDL_RENDERER_ACCELERATED | vsync);
  if (!evil_renderer)
    evil_renderer = SDL_CreateRenderer(evil_window, -1, vsync);
  if (!evil_renderer)
    evil_renderer = SDL_CreateRenderer(evil_window, -1, 0);
  if (!evil_renderer)
  {
    lprintf(LO_WARN, "evil_window_open: %s\n", SDL_GetError());
    SDL_DestroyWindow(evil_window);
    evil_window = NULL;
    return 0;
  }
  SDL_RenderSetLogicalSize(evil_renderer, w, h);
  evil_window_id = SDL_GetWindowID(evil_window);
  evil_event_count = 0;
  SDL_SetEventFilter(evil_event_filter, NULL);
  out[0] = SDL_GetRendererInfo(evil_renderer, &info) == 0 &&
           (info.flags & SDL_RENDERER_PRESENTVSYNC) != 0;
  out[1] = SDL_GetCurrentDisplayMode(SDL_GetWindowDisplayIndex(evil_window), &mode) == 0
               ? mode.refresh_rate
               : 0;
  return 1;
}

/* A picture shown (0RGB, a row of `w` after another): it waits for the screen's refresh where
 * the window has vsync. */
EVIL_API void evil_window_show(const unsigned int *px, int w, int h)
{
  if (!evil_renderer)
    return;
  if (!evil_texture || w != evil_texture_w || h != evil_texture_h)
  {
    if (evil_texture)
      SDL_DestroyTexture(evil_texture);
    evil_texture = SDL_CreateTexture(evil_renderer, SDL_PIXELFORMAT_RGB888,
                                     SDL_TEXTUREACCESS_STREAMING, w, h);
    evil_texture_w = w;
    evil_texture_h = h;
    SDL_RenderSetLogicalSize(evil_renderer, w, h);
  }
  if (!evil_texture)
    return;
  SDL_UpdateTexture(evil_texture, NULL, px, w * 4);
  SDL_SetRenderDrawColor(evil_renderer, 0, 0, 0, 255);
  SDL_RenderClear(evil_renderer);
  SDL_RenderCopy(evil_renderer, evil_texture, NULL, NULL);
  SDL_RenderPresent(evil_renderer);
}

/* The window's events since the last call, as pairs (kind, value: see evil_event_filter); how
 * many pairs. */
EVIL_API int evil_window_events(int *out, int max)
{
  int i, n;
  SDL_PumpEvents();
  n = evil_event_count < max ? evil_event_count : max;
  for (i = 0; i < n * 2; i++)
    out[i] = evil_events[i];
  evil_event_count = 0;
  return n;
}

EVIL_API void evil_window_title(const char *title)
{
  if (evil_window)
    SDL_SetWindowTitle(evil_window, title);
}

/* The window's icon (ARGB, a row of `w` after another): the title bar's and the task list's.
 * (On Windows the window has the program's own already; this is for where a window's icon is
 * the window's to give, X11 and Wayland.) */
EVIL_API void evil_window_icon(const unsigned int *px, int w, int h)
{
  SDL_Surface *icon;
  if (!evil_window)
    return;
  icon = SDL_CreateRGBSurfaceFrom((void *)px, w, h, 32, w * 4, 0x00ff0000, 0x0000ff00,
                                  0x000000ff, 0xff000000);
  if (!icon)
    return;
  SDL_SetWindowIcon(evil_window, icon); /* (copied: the pixels needn't outlive the call) */
  SDL_FreeSurface(icon);
}

EVIL_API void evil_window_close(void)
{
  SDL_SetEventFilter(NULL, NULL);
  if (evil_texture)
    SDL_DestroyTexture(evil_texture);
  if (evil_renderer)
    SDL_DestroyRenderer(evil_renderer);
  if (evil_window)
    SDL_DestroyWindow(evil_window);
  evil_texture = NULL;
  evil_renderer = NULL;
  evil_window = NULL;
  evil_window_id = 0;
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
  out->strength = p->powers[pw_strength] != 0;
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

/* The weapon in her hands (a weapontype_t she owns), brought up as at a level's start: what she
 * had up coming out of the level before, an episode's next level begun with it. */
EVIL_API void evil_ready(int weapon)
{
  player_t *p = &players[consoleplayer];
  if (weapon < 0 || weapon >= NUMWEAPONS || !p->weaponowned[weapon])
    return;
  p->readyweapon = weapon;
  P_SetupPsprites(p);
}
