/* Evil's way into DSDA-Doom: the game run a tic at a time, in-process, nothing drawn or
 * heard, and its state read straight out of memory (no copying the whole world into
 * objects every tic, as ViZDoom does).
 *
 * Built into a shared library with DSDA-Doom's own sources (see dsda/README.md). Plain C
 * types only across the boundary, so any toolchain can load it. One game a process: the
 * engine is all globals. */
#include <string.h>

#include "doomstat.h"
#include "d_event.h"
#include "d_main.h"
#include "d_player.h"
#include "g_game.h"
#include "m_menu.h"
#include "p_mobj.h"
#include "p_map.h"
#include "p_tick.h"
#include "r_state.h"
#include "i_system.h"
#include "i_video.h"
#include "lprintf.h"
#include "dsda/args.h"

#ifdef _WIN32
#define EVIL_API __declspec(dllexport)
#else
#define EVIL_API __attribute__((visibility("default")))
#endif

/* d_main.c: the game's own setup, without its main loop (added to DSDA-Doom by the patch). */
void D_DoomMainSetupForEvil(void);
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
} evil_player;

typedef struct {
  int gametic;
  int gamestate;   /* 0 level, 1 intermission, ... */
  int gameaction;
  int exited;      /* the level's been left (the exit used): stays set till a new game */
  int secret;      /* ...by the secret exit */
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
  return 0;
}

/* A new game: skill 1-5, the map (episode 1-4 and map 1-9; Doom II: episode 1, map 1-32),
 * monsters or none. Takes effect on the next tic. */
EVIL_API void evil_new_game(int skill, int episode, int map, int no_monsters)
{
  nomonsters = clnomonsters = no_monsters != 0;  /* (a new game resets the first from the second) */
  exited = secret_exit = 0;
  G_DeferedInitNew(skill - 1, episode, map);
}

EVIL_API void evil_god(int on)
{
  if (on)
    players[consoleplayer].cheats |= CF_GODMODE;
  else
    players[consoleplayer].cheats &= ~CF_GODMODE;
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
  I_StartTic();
  cmd = &local_cmds[consoleplayer][maketic % BACKUPTICS];
  G_BuildTiccmd(cmd);
  cmd->forwardmove = (signed char)forward;
  cmd->sidemove = (signed char)side;
  cmd->angleturn = (signed short)turn;
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
  maketic++;
}

EVIL_API void evil_get_status(evil_status *out)
{
  out->gametic = gametic;
  out->gamestate = gamestate;
  out->gameaction = gameaction;
  out->exited = exited;
  out->secret = secret_exit;
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
  if (p->mo)
  {
    out->x = fx(p->mo->x); out->y = fx(p->mo->y); out->z = fx(p->mo->z);
    out->angle = degrees(p->mo->angle);
    out->momx = fx(p->mo->momx); out->momy = fx(p->mo->momy); out->momz = fx(p->mo->momz);
  }
}
