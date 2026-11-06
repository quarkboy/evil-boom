/* Evil's way into DSDA-Doom: the game run a tic at a time, in-process, nothing drawn or
 * heard, and its state read straight out of memory (no copying the whole world into
 * objects every tic, as ViZDoom does).
 *
 * Built into a shared library with DSDA-Doom's own sources (see dsda/README.md). Plain C
 * types only across the boundary, so any toolchain can load it. */
#include <string.h>

#include "doomstat.h"
#include "d_main.h"
#include "d_player.h"
#include "g_game.h"
#include "m_menu.h"
#include "p_mobj.h"
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

/* d_main.c: the game's own setup, without its main loop (the one line added to DSDA-Doom). */
void D_DoomMainSetupForEvil(void);
void M_LoadDefaults(void);

typedef struct {
  unsigned long long id;  /* the thing's address: its own for as long as it's about */
  int type;        /* mobjtype_t */
  double x, y, z;
  double momx, momy, momz;
  double angle;    /* degrees, 0 = east, counter-clockwise */
  int health;
  unsigned long long flags;
} evil_mobj;

typedef struct {
  double x, y, z, angle, momx, momy;
  int health, armor, readyweapon, gametic;
  int weaponowned[9];
  int ammo[4];
  int playerstate;  /* 0 alive, 1 dead, 2 reborn */
} evil_player;

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

/* One tic, with this command for the player (forwardmove/sidemove as in a ticcmd: a run is
 * 50 forward, 40 sideways; turn: angleturn, <<16 for the angle; buttons: BT_ATTACK 1,
 * BT_USE 2, BT_CHANGE 4 with the weapon in bits 3-5). */
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
  gametic++;
  maketic++;
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

/* The things about (monsters, missiles, items, decorations), up to max. Returns how many
 * there are in all. */
EVIL_API int evil_mobjs(evil_mobj *out, int max)
{
  thinker_t *th;
  int n = 0;
  for (th = thinkercap.next; th != &thinkercap; th = th->next)
  {
    mobj_t *mo;
    if (th->function != P_MobjThinker)
      continue;
    mo = (mobj_t *)th;
    if (n < max)
    {
      evil_mobj *o = &out[n];
      o->id = (unsigned long long)(size_t)mo;
      o->type = mo->type;
      o->x = fx(mo->x); o->y = fx(mo->y); o->z = fx(mo->z);
      o->momx = fx(mo->momx); o->momy = fx(mo->momy); o->momz = fx(mo->momz);
      o->angle = degrees(mo->angle);
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
  out->gametic = gametic;
  out->playerstate = p->playerstate;
  out->health = p->health;
  out->armor = p->armorpoints[0];
  out->readyweapon = p->readyweapon;
  for (i = 0; i < 9 && i < NUMWEAPONS; i++)
    out->weaponowned[i] = p->weaponowned[i];
  for (i = 0; i < 4 && i < NUMAMMO; i++)
    out->ammo[i] = p->ammo[i];
  if (p->mo)
  {
    out->x = fx(p->mo->x); out->y = fx(p->mo->y); out->z = fx(p->mo->z);
    out->angle = degrees(p->mo->angle);
    out->momx = fx(p->mo->momx); out->momy = fx(p->mo->momy);
  }
}
