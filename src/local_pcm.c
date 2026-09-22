/* Experimental second audio sink: queued SDL PCM, slaved to video timestamps.
 * SDL's queued-byte count is NOT a measurement of hardware output latency.
 * All output and lifecycle operations run on the app thread; no SDL callback.
 */
#include "local_audio.h"
#include <stdlib.h>
#include <string.h>
#ifndef PLX_LOCAL_AUDIO
#define PLX_LOCAL_AUDIO 0
#endif
#if PLX_LOCAL_AUDIO
#include <dlfcn.h>
#include <SDL2/SDL_audio.h>
#include <SDL2/SDL.h>
#define FN(name) static __typeof__(&name) p_##name
FN(SDL_InitSubSystem); FN(SDL_QuitSubSystem); FN(SDL_OpenAudioDevice); FN(SDL_CloseAudioDevice);
FN(SDL_PauseAudioDevice); FN(SDL_ClearQueuedAudio); FN(SDL_GetQueuedAudioSize); FN(SDL_QueueAudio);
FN(SDL_GetCurrentAudioDriver);
static int resolved;
static int resolve(void) {
    if (resolved) return resolved>0;
    resolved=-1;
#define LOAD(name) do { *(void **)(&p_##name)=dlsym(RTLD_DEFAULT,#name); if(!p_##name) return 0; } while(0)
    LOAD(SDL_InitSubSystem); LOAD(SDL_QuitSubSystem); LOAD(SDL_OpenAudioDevice); LOAD(SDL_CloseAudioDevice);
    LOAD(SDL_PauseAudioDevice); LOAD(SDL_ClearQueuedAudio); LOAD(SDL_GetQueuedAudioSize); LOAD(SDL_QueueAudio);
    LOAD(SDL_GetCurrentAudioDriver);
#undef LOAD
    resolved=1; return 1;
}
struct Sink { SDL_AudioDeviceID dev; int paused; };
void *plx_pcm_open(void) {
    if(!resolve() || p_SDL_InitSubSystem(SDL_INIT_AUDIO)<0) return NULL;
    const char *driver=p_SDL_GetCurrentAudioDriver();
#ifndef PLX_AUDIO_TEST_DUMMY
    if(!driver || !strcmp(driver,"dummy") || !strcmp(driver,"disk")) {
        p_SDL_QuitSubSystem(SDL_INIT_AUDIO); return NULL;
    }
#else
    (void)driver;
#endif
    SDL_AudioSpec want,got;
    memset(&want,0,sizeof(want)); memset(&got,0,sizeof(got));
    want.freq=48000; want.format=AUDIO_S16LSB; want.channels=2; want.samples=1024;
    SDL_AudioDeviceID dev=p_SDL_OpenAudioDevice(NULL,0,&want,&got,0);
    if(!dev || got.freq!=48000 || got.format!=AUDIO_S16LSB || got.channels!=2) {
        if(dev) p_SDL_CloseAudioDevice(dev);
        p_SDL_QuitSubSystem(SDL_INIT_AUDIO); return NULL;
    }
    struct Sink *s=calloc(1,sizeof(*s));
    if(!s) {p_SDL_CloseAudioDevice(dev);p_SDL_QuitSubSystem(SDL_INIT_AUDIO);return NULL;}
    s->dev=dev; s->paused=1; return s;
}
void plx_pcm_close(void *opaque) {
    struct Sink *s=opaque; if(!s) return;
    p_SDL_PauseAudioDevice(s->dev,1);p_SDL_ClearQueuedAudio(s->dev);
    p_SDL_CloseAudioDevice(s->dev);p_SDL_QuitSubSystem(SDL_INIT_AUDIO);free(s);
}
void plx_pcm_pause(void *opaque,int paused) {
    struct Sink *s=opaque;if(!s) return;
    paused=!!paused;
    if(paused!=s->paused) {p_SDL_PauseAudioDevice(s->dev,paused);s->paused=paused;}
}
void plx_pcm_flush(void *opaque) {
    struct Sink *s=opaque;if(!s) return;
    plx_pcm_pause(s,1);p_SDL_ClearQueuedAudio(s->dev);
}
unsigned plx_pcm_queued(void *opaque) {
    struct Sink *s=opaque;return s?p_SDL_GetQueuedAudioSize(s->dev):0;
}
int plx_pcm_feed(void *opaque,const uint8_t *data,int bytes,int64_t pts,int64_t video,int priming) {
    struct Sink *s=opaque;
    if(!s || !data || bytes<=0 || bytes>65536*4 || bytes%4 ||
       pts < -3600LL*1000000000LL || pts > 604800LL*1000000000LL ||
       video < -3600LL*1000000000LL || video > 604800LL*1000000000LL) return -1;
    const unsigned bound=48000; /* 250 ms. Large decoded frames are split by the Rust producer. */
    unsigned queued=p_SDL_GetQueuedAudioSize(s->dev);
    if((unsigned)bytes>bound) return -1;
    if(queued>bound-(unsigned)bytes) return 0;
    int64_t expected=video+(int64_t)queued*1000000000LL/192000;
    /* The decoder hands over finite chunks of at most 20 ms. Hold early audio, trim late audio.
     * No extrapolation from wall time, and no advancing the clock while video is held. */
    int64_t tolerance=priming?20000000LL:40000000LL;
    if(pts>expected+tolerance) return 0;
    if(pts<expected-80000000LL) {
        int64_t late=expected-pts;
        int64_t frames=bytes/4;
        // Reject a fully late chunk before multiplying: valid-but-distant timestamps
        // can differ by days, so late * sample_rate alone could overflow int64_t.
        if(late >= (frames*1000000000LL + 47999)/48000) return 1;
        int64_t skip=late*48000/1000000000LL;
        if(skip>=frames) return 1;
        data+=skip*4;bytes-=(int)(skip*4);
    }
    return p_SDL_QueueAudio(s->dev,data,(Uint32)bytes)==0?1:-1;
}
#else
void *plx_pcm_open(void){return NULL;}
void plx_pcm_close(void *p){(void)p;}
void plx_pcm_pause(void *p,int v){(void)p;(void)v;}
void plx_pcm_flush(void *p){(void)p;}
unsigned plx_pcm_queued(void *p){(void)p;return 0;}
int plx_pcm_feed(void *p,const uint8_t *d,int n,int64_t t,int64_t v,int b){(void)p;(void)d;(void)n;(void)t;(void)v;(void)b;return -1;}
#endif
