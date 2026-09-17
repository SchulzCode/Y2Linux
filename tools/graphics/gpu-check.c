// SPDX-License-Identifier: MIT
/* Standard API platform qualification, not the Reborn application. */
#define _POSIX_C_SOURCE 200809L
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES2/gl2.h>
#include <gbm.h>
#include <xf86drm.h>
#include <xf86drmMode.h>
#include <drm_fourcc.h>
#include <lima_drm.h>
#include <errno.h>
#include <fcntl.h>
#include <math.h>
#include <poll.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <time.h>
#include <unistd.h>

static volatile sig_atomic_t stopping, redraw;
static void interrupted(int sig) { if (sig == SIGUSR1) redraw = 1; else stopping = 1; }
static double now(void)
{
 struct timespec ts;
 clock_gettime(CLOCK_MONOTONIC, &ts);
 return ts.tv_sec + ts.tv_nsec / 1e9;
}
static void pace(double until)
{
 struct timespec ts = { .tv_sec = (time_t)until,
                       .tv_nsec = (long)((until - floor(until)) * 1e9) };
 while (!stopping && clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &ts, NULL) == EINTR) {}
}
static int open_driver(const char *wanted, bool render)
{
 for (unsigned i = 0; i < 16; i++) {
  char path[64];
  snprintf(path, sizeof(path), "/dev/dri/%s%u", render ? "renderD" : "card", i + (render ? 128 : 0));
  int fd = open(path, O_RDWR | O_CLOEXEC);
  if (fd < 0) continue;
  drmVersionPtr version = drmGetVersion(fd);
  bool found = version && version->name && !strcmp(version->name, wanted);
  drmFreeVersion(version);
  if (found) { printf("DRM_%s=%s driver=%s\n", render ? "RENDER" : "KMS", path, wanted); return fd; }
  close(fd);
 }
 fprintf(stderr, "Required DRM driver %s not found\n", wanted);
 return -1;
}
static int lima_identity(void)
{
 int fd = open_driver("lima", true);
 if (fd < 0) return -1;
 const unsigned params[] = { DRM_LIMA_PARAM_GPU_ID, DRM_LIMA_PARAM_NUM_PP,
                             DRM_LIMA_PARAM_GP_VERSION, DRM_LIMA_PARAM_PP_VERSION };
 const char *names[] = { "GPU_ID", "PP_CORES", "GP_VERSION", "PP_VERSION" };
 for (unsigned i = 0; i < 4; i++) {
  struct drm_lima_get_param p = { .param = params[i] };
  if (drmIoctl(fd, DRM_IOCTL_LIMA_GET_PARAM, &p)) { perror("Lima identity"); close(fd); return -1; }
  printf("LIMA_%s=0x%llx\n", names[i], (unsigned long long)p.value);
  if ((i == 0 && p.value != DRM_LIMA_PARAM_GPU_ID_MALI400) || (i == 1 && p.value != 2)) {
   fprintf(stderr, "Unexpected GPU: require actual Mali-400 MP2\n"); close(fd); return -1;
  }
 }
 close(fd);
 return 0;
}
struct fb { uint32_t id; int fd; };
static void destroy_fb(struct gbm_bo *bo, void *data)
{
 (void)bo;
 struct fb *fb = data;
 drmModeRmFB(fb->fd, fb->id);
 free(fb);
}
static struct fb *framebuffer(int fd, struct gbm_bo *bo)
{
 struct fb *fb = gbm_bo_get_user_data(bo);
 if (fb) return fb;
 if (gbm_bo_get_modifier(bo) != DRM_FORMAT_MOD_LINEAR &&
     gbm_bo_get_modifier(bo) != DRM_FORMAT_MOD_INVALID) {
  fprintf(stderr, "KMS requires linear scanout\n"); return NULL;
 }
 fb = calloc(1, sizeof(*fb));
 if (!fb) return NULL;
 fb->fd = fd;
 uint32_t handles[4] = { gbm_bo_get_handle(bo).u32 };
 uint32_t pitches[4] = { gbm_bo_get_stride(bo) }, offsets[4] = {0};
 if (drmModeAddFB2(fd, gbm_bo_get_width(bo), gbm_bo_get_height(bo),
                   gbm_bo_get_format(bo), handles, pitches, offsets, &fb->id, 0)) {
  perror("drmModeAddFB2"); free(fb); return NULL;
 }
 gbm_bo_set_user_data(bo, fb, destroy_fb);
 return fb;
}
struct flip { bool waiting; unsigned count, sequence; double timestamp; };
static void flipped(int fd, unsigned seq, unsigned sec, unsigned usec, void *data)
{
 (void)fd;
 struct flip *f = data;
 f->waiting = false; f->count++; f->sequence = seq;
 f->timestamp = sec + usec / 1e6;
}
static int wait_flip(int fd, struct flip *f)
{
 double deadline = now() + 3;
 drmEventContext events = { .version = DRM_EVENT_CONTEXT_VERSION, .page_flip_handler = flipped };
 while (f->waiting) {
  struct pollfd pollfd = { .fd = fd, .events = POLLIN };
  int ms = (int)((deadline - now()) * 1000);
  if (ms <= 0) { fprintf(stderr, "KMS page-flip timeout\n"); return -1; }
  int ret = poll(&pollfd, 1, ms);
  if (ret < 0 && errno == EINTR) continue;
  if (ret <= 0 || !(pollfd.revents & POLLIN)) { fprintf(stderr, "KMS event failure\n"); return -1; }
  if (drmHandleEvent(fd, &events)) return -1;
 }
 return 0;
}
static GLuint shader(GLenum type, const char *source)
{
 GLuint s = glCreateShader(type);
 glShaderSource(s, 1, &source, NULL); glCompileShader(s);
 GLint ok; glGetShaderiv(s, GL_COMPILE_STATUS, &ok);
 if (!ok) { char log[1024]; glGetShaderInfoLog(s, sizeof(log), NULL, log); fprintf(stderr, "shader: %s\n", log); glDeleteShader(s); return 0; }
 return s;
}
static GLuint program(void)
{
 const char *vs = "attribute vec2 p; attribute vec2 uv; varying vec2 t;"
                  "uniform vec4 box; uniform float angle;"
                  "void main(){vec2 q=p*box.zw; float c=cos(angle),s=sin(angle);"
                  "gl_Position=vec4(c*q.x-s*q.y+box.x,s*q.x+c*q.y+box.y,0.,1.);t=uv;}";
 const char *fs = "precision mediump float; varying vec2 t; uniform sampler2D tex;"
                  "uniform vec4 tint; uniform vec2 scroll;"
                  "void main(){gl_FragColor=texture2D(tex,t+scroll)*tint;}";
 GLuint v = shader(GL_VERTEX_SHADER, vs), f = shader(GL_FRAGMENT_SHADER, fs);
 if (!v || !f) { if (v) glDeleteShader(v); if (f) glDeleteShader(f); return 0; }
 GLuint p = glCreateProgram(); glAttachShader(p, v); glAttachShader(p, f);
 glBindAttribLocation(p, 0, "p"); glBindAttribLocation(p, 1, "uv"); glLinkProgram(p);
 glDeleteShader(v); glDeleteShader(f);
 GLint ok; glGetProgramiv(p, GL_LINK_STATUS, &ok);
 if (!ok) { char log[1024]; glGetProgramInfoLog(p, sizeof(log), NULL, log); fprintf(stderr, "program: %s\n", log); glDeleteProgram(p); return 0; }
 return p;
}
static void quad(GLuint p, float x, float y, float w, float h, float a,
                 float r, float g, float b, float alpha, float sy)
{
 glUniform4f(glGetUniformLocation(p, "box"), x, y, w, h);
 glUniform1f(glGetUniformLocation(p, "angle"), a);
 glUniform4f(glGetUniformLocation(p, "tint"), r, g, b, alpha);
 glUniform2f(glGetUniformLocation(p, "scroll"), 0, sy);
 glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);
}
static void draw(GLuint p, GLuint texture, GLuint white, float t)
{
 glClearColor(.035f, .055f, .09f, 1); glClear(GL_COLOR_BUFFER_BIT);
 glBindTexture(GL_TEXTURE_2D, texture);
 /* Moving/scaling album-art-like texture and textured geometry. */
 quad(p, -.28f, .17f, .47f + .035f*sinf(t), .55f, .12f*sinf(t), 1,1,1,1,0);
 quad(p, .5f, .43f, .22f, .29f, t*.4f, 1,.85f,.65f,.92f,0);
 /* Scrolling translucent layers, composed on the GPU. */
 for (int i=0;i<4;i++)
  quad(p, .30f, -.15f-i*.17f, .60f, .068f, 0, .3f+i*.13f,.68f,.9f,.48f,t*.05f+i*.1f);
 glBindTexture(GL_TEXTURE_2D, white);
 quad(p, -.60f+sinf(t)*.12f, -.70f, .28f, .14f, 0, .04f,.08f,.13f,.78f,0);
 /* Unambiguous color/alpha references for physical visual review. */
 for (int i=0;i<3;i++) quad(p, -.88f+i*.13f,.87f,.055f,.07f,0,i==0,i==1,i==2,1,0);
 quad(p, .76f,-.77f,.12f,.12f,0,1,1,1,.5f,0);
}
static GLuint make_texture(bool white)
{
 unsigned char pixels[128*128*4];
 int side = white ? 1 : 128;
 for (int y=0;y<side;y++) for (int x=0;x<side;x++) {
  unsigned char *p = pixels+(y*side+x)*4;
  bool check = ((x/16)^(y/16))&1;
  p[0]=white?255:40+x; p[1]=white?255:60+y; p[2]=white?255:check?220:75; p[3]=255;
 }
 GLuint tex; glGenTextures(1,&tex); glBindTexture(GL_TEXTURE_2D,tex);
 glTexImage2D(GL_TEXTURE_2D,0,GL_RGBA,side,side,0,GL_RGBA,GL_UNSIGNED_BYTE,pixels);
 glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_LINEAR);
 glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_LINEAR);
 glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_S,GL_REPEAT);
 glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_T,GL_REPEAT);
 return tex;
}
static int compare_double(const void *a, const void *b)
{ double x=*(const double *)a,y=*(const double *)b; return (x>y)-(x<y); }
static unsigned number(const char *s, unsigned max)
{
 char *end; errno=0; unsigned long v=strtoul(s,&end,10);
 if (errno || !*s || *end || !v || v>max) return 0;
 return (unsigned)v;
}
int main(int argc, char **argv)
{
 unsigned seconds=20,fps=30; bool static_ui=false;
 for (int i=1;i<argc;i++) {
  if (!strcmp(argv[i],"--help")) {
   puts("y2-gpu-check [--seconds 1..300] [--fps 1..60] [--static]\n"
        "Native 480x360 Lima/GBM/EGL/GLES2 qualification. Default 20s/30fps.\n"
        "--static draws once, then sleeps; SIGUSR1 requests one redraw.\n"
        "SIGINT/SIGTERM restores KMS and exits. No software renderer accepted."); return 0;
  } else if (!strcmp(argv[i],"--static")) static_ui=true;
  else if (!strcmp(argv[i],"--seconds") && i+1<argc) seconds=number(argv[++i],300);
  else if (!strcmp(argv[i],"--fps") && i+1<argc) fps=number(argv[++i],60);
  else { fprintf(stderr,"Invalid option; use --help\n"); return 2; }
 }
 if (!seconds || !fps) { fprintf(stderr,"Duration/FPS outside bounded range\n"); return 2; }
 setvbuf(stdout,NULL,_IOLBF,0);
 struct sigaction sa={.sa_handler=interrupted}; sigemptyset(&sa.sa_mask);
 sigaction(SIGINT,&sa,NULL);sigaction(SIGTERM,&sa,NULL);sigaction(SIGUSR1,&sa,NULL);
 if (lima_identity()) return 1;
 int fd=open_driver("mediatek",false), result=1;
 if (fd<0) return 1;
 drmModeRes *resources=NULL; drmModeConnector *connector=NULL; drmModeCrtc *old=NULL;
 struct gbm_device *gbm=NULL; struct gbm_surface *surface=NULL; struct gbm_bo *current=NULL;
 EGLDisplay display=EGL_NO_DISPLAY; EGLSurface window=EGL_NO_SURFACE; EGLContext context=EGL_NO_CONTEXT;
 GLuint prog=0,texture=0,white=0,vbo=0; bool modeset=false;
 uint32_t crtc=0; drmModeModeInfo mode={0}; struct flip flip={0};
 double samples[18002]; unsigned count=0;
 if (drmSetMaster(fd)) { perror("drmSetMaster"); goto out; }
 resources=drmModeGetResources(fd);
 if (!resources) goto out;
 for (int i=0;i<resources->count_connectors;i++) {
  drmModeConnector *c=drmModeGetConnector(fd,resources->connectors[i]);
  if (c && c->connection==DRM_MODE_CONNECTED && c->count_modes) { connector=c; break; }
  drmModeFreeConnector(c);
 }
 if (!connector) { fprintf(stderr,"No connected KMS panel\n"); goto out; }
 for (int i=0;i<connector->count_modes;i++) if (connector->modes[i].hdisplay==480 && connector->modes[i].vdisplay==360) { mode=connector->modes[i]; break; }
 if (!mode.hdisplay) { fprintf(stderr,"Required native 480x360 mode missing\n"); goto out; }
 drmModeEncoder *encoder=drmModeGetEncoder(fd,connector->encoder_id);
 if (encoder) { crtc=encoder->crtc_id; drmModeFreeEncoder(encoder); }
 if (!crtc) {
  for (int i=0;i<connector->count_encoders && !crtc;i++) {
   encoder=drmModeGetEncoder(fd,connector->encoders[i]);
   if (!encoder) continue;
   for (int j=0;j<resources->count_crtcs;j++) if (encoder->possible_crtcs & (1U<<j)) { crtc=resources->crtcs[j]; break; }
   drmModeFreeEncoder(encoder);
  }
 }
 if (!crtc || !(old=drmModeGetCrtc(fd,crtc))) goto out;
 gbm=gbm_create_device(fd); if (!gbm) goto out;
 if (!gbm_device_is_format_supported(gbm,GBM_FORMAT_XRGB8888,GBM_BO_USE_SCANOUT|GBM_BO_USE_RENDERING|GBM_BO_USE_LINEAR)) {
  fprintf(stderr,"Required linear XRGB8888 GBM scanout unsupported\n"); goto out;
 }
 surface=gbm_surface_create(gbm,480,360,GBM_FORMAT_XRGB8888,GBM_BO_USE_SCANOUT|GBM_BO_USE_RENDERING|GBM_BO_USE_LINEAR);
 if (!surface) goto out;
 PFNEGLGETPLATFORMDISPLAYEXTPROC get_display=(PFNEGLGETPLATFORMDISPLAYEXTPROC)eglGetProcAddress("eglGetPlatformDisplayEXT");
 if (!get_display) goto out;
 display=get_display(EGL_PLATFORM_GBM_KHR,gbm,NULL);
 EGLint major,minor;
 if (display==EGL_NO_DISPLAY || !eglInitialize(display,&major,&minor) || !eglBindAPI(EGL_OPENGL_ES_API)) goto out;
 printf("EGL_VENDOR=%s\nEGL_VERSION=%s\nEGL_APIS=%s\n",eglQueryString(display,EGL_VENDOR),eglQueryString(display,EGL_VERSION),eglQueryString(display,EGL_CLIENT_APIS));
 const EGLint attributes[]={EGL_SURFACE_TYPE,EGL_WINDOW_BIT,EGL_RENDERABLE_TYPE,EGL_OPENGL_ES2_BIT,
  EGL_RED_SIZE,8,EGL_GREEN_SIZE,8,EGL_BLUE_SIZE,8,EGL_DEPTH_SIZE,0,EGL_STENCIL_SIZE,0,EGL_NONE};
 EGLConfig configs[64],config=NULL; EGLint n;
 if (!eglChooseConfig(display,attributes,configs,64,&n)) goto out;
 for (int i=0;i<n;i++) { EGLint visual; eglGetConfigAttrib(display,configs[i],EGL_NATIVE_VISUAL_ID,&visual); if (visual==(EGLint)GBM_FORMAT_XRGB8888) { config=configs[i];break; } }
 if (!config) { fprintf(stderr,"EGL XRGB8888 ES2 config missing\n"); goto out; }
 const EGLint context_attributes[]={EGL_CONTEXT_CLIENT_VERSION,2,EGL_NONE};
 context=eglCreateContext(display,config,EGL_NO_CONTEXT,context_attributes);
 window=eglCreateWindowSurface(display,config,(EGLNativeWindowType)surface,NULL);
 if (context==EGL_NO_CONTEXT || window==EGL_NO_SURFACE || !eglMakeCurrent(display,window,window,context)) goto out;
 const char *renderer=(const char *)glGetString(GL_RENDERER);
 printf("GL_VENDOR=%s\nGL_RENDERER=%s\nGL_VERSION=%s\n",glGetString(GL_VENDOR),renderer?renderer:"(null)",glGetString(GL_VERSION));
 if (!renderer || !strstr(renderer,"Mali400") || strstr(renderer,"llvmpipe") || strstr(renderer,"softpipe")) { fprintf(stderr,"Hardware Mali400 renderer required\n"); goto out; }
 GLint limit,units; glGetIntegerv(GL_MAX_TEXTURE_SIZE,&limit);glGetIntegerv(GL_MAX_TEXTURE_IMAGE_UNITS,&units);
 printf("GL_MAX_TEXTURE_SIZE=%d texture_units=%d\nGL_EXTENSIONS=%s\n",limit,units,glGetString(GL_EXTENSIONS));
 EGLint id,alpha; eglGetConfigAttrib(display,config,EGL_CONFIG_ID,&id);eglGetConfigAttrib(display,config,EGL_ALPHA_SIZE,&alpha);
 printf("EGL_CONFIG=%d scanout=XRGB8888 linear alpha_bits=%d texture_alpha=RGBA8888 GLES_blending=enabled\n",id,alpha);
 if (!eglSwapInterval(display,0)) goto out; /* Actual pacing is the KMS event. */
 prog=program();if (!prog) goto out;
 glUseProgram(prog);glUniform1i(glGetUniformLocation(prog,"tex"),0);
 const GLfloat vertices[]={-1,-1,0,1, 1,-1,1,1, -1,1,0,0, 1,1,1,0};
 glGenBuffers(1,&vbo);glBindBuffer(GL_ARRAY_BUFFER,vbo);glBufferData(GL_ARRAY_BUFFER,sizeof(vertices),vertices,GL_STATIC_DRAW);
 glVertexAttribPointer(0,2,GL_FLOAT,GL_FALSE,4*sizeof(GLfloat),(void *)0);
 glVertexAttribPointer(1,2,GL_FLOAT,GL_FALSE,4*sizeof(GLfloat),(void *)(2*sizeof(GLfloat)));
 glEnableVertexAttribArray(0);glEnableVertexAttribArray(1);
 glViewport(0,0,480,360);glEnable(GL_BLEND);glBlendFunc(GL_SRC_ALPHA,GL_ONE_MINUS_SRC_ALPHA);
 texture=make_texture(false);white=make_texture(true);
 struct rusage before,after;getrusage(RUSAGE_SELF,&before);
 double start=now(),last=start,next=start;
 redraw=1;
 printf("PRESENTATION=GBM/kmsro PRIME dma-buf -> existing Mediatek KMS; no CPU copy\nMODE=480x360 refresh=%d fps_cap=%u static=%u\n",mode.vrefresh,fps,static_ui);
 while (!stopping && now()-start<seconds) {
  if (static_ui && !redraw) { pace(now()+.1);continue; }
  redraw=0;double frame_start=now();
  draw(prog,texture,white,(float)(frame_start-start));
  GLenum error=glGetError();
  if (error!=GL_NO_ERROR) { fprintf(stderr,"GL error=0x%x\n",error);goto out; }
  if (!eglSwapBuffers(display,window)) goto out;
  struct gbm_bo *bo=gbm_surface_lock_front_buffer(surface);
  if (!bo) goto out;
  struct fb *fb=framebuffer(fd,bo);
  if (!fb) { gbm_surface_release_buffer(surface,bo);goto out; }
  if (!modeset) {
   if (drmModeSetCrtc(fd,crtc,fb->id,0,0,&connector->connector_id,1,&mode)) { perror("drmModeSetCrtc");gbm_surface_release_buffer(surface,bo);goto out; }
   modeset=true;
  } else {
   flip.waiting=true;
   if (drmModePageFlip(fd,crtc,fb->id,DRM_MODE_PAGE_FLIP_EVENT,&flip)) { perror("drmModePageFlip");gbm_surface_release_buffer(surface,bo);goto out; }
   if (wait_flip(fd,&flip)) { /* Stop scanout before returning an uncertain buffer. */
    drmModeSetCrtc(fd,crtc,0,0,0,NULL,0,NULL);gbm_surface_release_buffer(surface,bo);goto out;
   }
  }
  if (current) gbm_surface_release_buffer(surface,current);
  current=bo;
  double end=now(); if (count<18002) samples[count++]=(end-frame_start)*1000;
  if (static_ui) printf("STATIC_READY frames=%u GPU may runtime-idle; SIGUSR1 redraws\n",count);
  if (end-last>=2) { printf("PROGRESS frames=%u flips=%u last_sequence=%u elapsed=%.3f\n",count,flip.count,flip.sequence,end-start);last=end; }
  next+=1.0/fps;if(next<end)next=end;pace(next);
 }
 getrusage(RUSAGE_SELF,&after);
 double elapsed=now()-start, cpu=(after.ru_utime.tv_sec-before.ru_utime.tv_sec)+(after.ru_stime.tv_sec-before.ru_stime.tv_sec)+
  (after.ru_utime.tv_usec-before.ru_utime.tv_usec+after.ru_stime.tv_usec-before.ru_stime.tv_usec)/1e6;
 qsort(samples,count,sizeof(samples[0]),compare_double);
 printf("RESULT frames=%u page_flips=%u elapsed_s=%.3f fps=%.2f submit_present_ms_p50=%.3f p95=%.3f max=%.3f cpu_pct=%.2f maxrss_kib=%ld\n",
  count,flip.count,elapsed,count/elapsed,count?samples[count/2]:0,count?samples[(count-1)*95/100]:0,count?samples[count-1]:0,100*cpu/elapsed,after.ru_maxrss);
 result=count?0:1;
out:
 if (result) fprintf(stderr,"Qualification failed; EGL error=0x%x errno=%d\n",eglGetError(),errno);
 if (modeset && old) {
  int ret=old->mode_valid?drmModeSetCrtc(fd,old->crtc_id,old->buffer_id,old->x,old->y,&connector->connector_id,1,&old->mode):drmModeSetCrtc(fd,crtc,0,0,0,NULL,0,NULL);
  if (ret) { perror("restore KMS"); drmModeSetCrtc(fd,crtc,0,0,0,NULL,0,NULL);result=1; }
 }
 if (context!=EGL_NO_CONTEXT && display!=EGL_NO_DISPLAY) {
  if (vbo) glDeleteBuffers(1,&vbo);
  if (texture) glDeleteTextures(1,&texture);
  if (white) glDeleteTextures(1,&white);
  if (prog) glDeleteProgram(prog);
 }
 if (current)gbm_surface_release_buffer(surface,current);
 if (display!=EGL_NO_DISPLAY) {
  eglMakeCurrent(display,EGL_NO_SURFACE,EGL_NO_SURFACE,EGL_NO_CONTEXT);
  if(window!=EGL_NO_SURFACE)eglDestroySurface(display,window);
  if(context!=EGL_NO_CONTEXT)eglDestroyContext(display,context);
  eglTerminate(display);
 }
 if (surface) gbm_surface_destroy(surface);
 if (gbm) gbm_device_destroy(gbm);
 drmModeFreeCrtc(old);drmModeFreeConnector(connector);drmModeFreeResources(resources);
 drmDropMaster(fd);close(fd);
 printf("EXIT status=%d KMS_restored=%u\n",result,modeset);
 return result;
}
