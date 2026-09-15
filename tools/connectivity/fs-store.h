/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_FS_STORE_H
#define Y2_FS_STORE_H
/* Modem-only RAM filesystem. No operation resolves a modem name on the host.
 * Seeded factory records retain an immutable copy; writes affect RAM shadows.
 * Limits apply independently to records, handles, file size and total bytes. */
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <fnmatch.h>
#include "../../kernel/platform/connectivity/protocol.h"
#define Y2_FS_FILES 2048
#define Y2_FS_HANDLES 129
#define Y2_FS_FILE_LIMIT (1024U * 1024)
#define Y2_FS_QUOTA (16U * 1024 * 1024)
struct y2_file {
	char name[132];
	unsigned char *data, *factory;
	unsigned size, factory_size, directory, refs;
};
struct y2_handle {
	struct y2_file *file;
	unsigned position, flags, search, next, attr, mask;
	char pattern[132];
};
struct y2_store {
	struct y2_file files[Y2_FS_FILES];
	struct y2_handle handles[Y2_FS_HANDLES];
	unsigned used, shadow_writes, mutations;
};
struct y2_reply { unsigned char *data; unsigned size, count; };
static void y2_fs_arg(struct y2_reply *r, const void *p, unsigned n)
{
	unsigned padded = (n + 3) & ~3U;
	/* Callers preflight all lengths; no partial/truncated wire response. */
	if (n > Y2_FS_STRIDE || r->size + 4 + padded > Y2_FS_STRIDE) abort();
	y2_conn_put32(r->data + r->size, n); r->size += 4;
	if (n) memcpy(r->data + r->size, p, n);
	memset(r->data + r->size + n, 0, padded - n); r->size += padded;
	y2_conn_put32(r->data + 4, ++r->count);
}
static void y2_fs_int(struct y2_reply *r, int n)
{
	unsigned char b[4]; y2_conn_put32(b, n); y2_fs_arg(r, b, 4);
}
static int y2_fs_word(const struct y2_fs_packet *p, unsigned i, unsigned *v)
{
	if (i >= p->count || p->args[i].size != 4) return -2;
	*v = y2_conn_le32(p->args[i].data); return 0;
}
static int y2_fs_name(const struct y2_fs_packet *p, unsigned i, char *out, int pattern)
{
	unsigned n, component = 0, at = 2;
	const unsigned char *b;
	if (i >= p->count) return -2;
	n = p->args[i].size; b = p->args[i].data;
	if (n < 6 || n > 256 || (n & 1) || !strchr("ZXYW", b[0]) ||
	    !b[0] || b[1] || b[2] != ':' || b[3]) return -3;
	out[0] = b[0]; out[1] = '/';
	unsigned start = b[4] == '\\' && !b[5] ? 6 : 4;
	for (unsigned j = start; j < n; j += 2) {
		unsigned c = b[j];
		if (b[j + 1]) return -3;
		if (!c) {
			if (j + 2 != n) return -3;
			if (!component && at > 2) at--; /* trailing directory slash */
			out[at] = 0; return 0;
		}
		if (c == '\\') {
			if (!component) return -3;
			out[at++] = '/'; component = 0; continue;
		}
		if (!((c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') ||
		      (c >= '0' && c <= '9') || c == '_' ||
		      (pattern && (c == '*' || c == '?')) || (c == '.' && component))) return -3;
		/* Two successive dots are not accepted, including '..' components. */
		if (c == '.' && out[at - 1] == '.') return -3;
		if (c >= 'a' && c <= 'z') c -= 'a' - 'A';
		if (at + 1 >= 132) return -3;
		out[at++] = c; component++;
	}
	return -3;
}
static struct y2_file *y2_fs_lookup(struct y2_store *s, const char *name)
{
	for (unsigned i = 0; i < Y2_FS_FILES; i++)
		if (s->files[i].name[0] && !strcmp(s->files[i].name, name)) return &s->files[i];
	return NULL;
}
static int y2_fs_resize(struct y2_store *s, struct y2_file *f, unsigned n)
{
	if (n > Y2_FS_FILE_LIMIT) return -38;
	if (n > f->size && n - f->size > Y2_FS_QUOTA - s->used) return -22;
	if (n != f->size) {
		unsigned char *p = n ? calloc(1, n) : NULL;
		if (n && !p) return -44;
		if (n && f->size) memcpy(p, f->data, n < f->size ? n : f->size);
		if (f->data) { memset(f->data, 0, f->size); free(f->data); }
		f->data = p;
	}
	s->used = s->used - f->size + n; f->size = n;
	return 0;
}
static struct y2_file *y2_fs_new(struct y2_store *s, const char *name, unsigned directory)
{
	for (unsigned i = 0; i < Y2_FS_FILES; i++) if (!s->files[i].name[0] && !s->files[i].refs) {
		struct y2_file *f = &s->files[i];
		y2_fs_resize(s, f, 0); free(f->data); free(f->factory); memset(f, 0, sizeof(*f));
		strcpy(f->name, name); f->directory = directory; return f;
	}
	return NULL;
}
static int y2_fs_parent(struct y2_store *s, const char *name)
{
	char parent[132]; strcpy(parent, name); char *p = strrchr(parent, '/');
	if (!p || !p[1]) return -16;
	if (p == parent + 1) p[1] = 0; else *p = 0;
	struct y2_file *f = y2_fs_lookup(s, parent);
	return f && f->directory ? 0 : -19;
}
static int y2_fs_handle(struct y2_store *s)
{
	for (unsigned i = 1; i < Y2_FS_HANDLES; i++)
		if (!s->handles[i].file && !s->handles[i].search) return i;
	return -5;
}
static void y2_fs_close(struct y2_store *s, unsigned h)
{
	if (s->handles[h].file) s->handles[h].file->refs--;
	memset(&s->handles[h], 0, sizeof(s->handles[h]));
}
static int y2_fs_open(struct y2_store *s, const char *name, unsigned flags)
{
	struct y2_file *f = y2_fs_lookup(s, name); int h = y2_fs_handle(s), ret;
	if (h < 0) return h;
	if ((ret = y2_fs_parent(s, name))) return ret;
	if (strchr(name, '*') || strchr(name, '?')) return -3;
	if (!f && (flags & 0x30000)) f = y2_fs_new(s, name, 0);
	if (!f) return flags & 0x30000 ? -22 : -9;
	if (f->directory) return -16;
	if (flags & 0x20000) {
		if (flags & 0x100) return -16;
		y2_fs_resize(s, f, 0);
		if (f->factory) s->shadow_writes++;
	}
	s->handles[h].file = f; s->handles[h].flags = flags; f->refs++;
	return h;
}
static int y2_fs_seed(struct y2_store *s, const char *name, const void *data, unsigned size)
{
	if (y2_fs_lookup(s, name) || size > 2060) return -1;
	struct y2_file *f = y2_fs_new(s, name, 0);
	if (!f || y2_fs_resize(s, f, size)) return -1;
	f->factory = malloc(size ? size : 1);
	if (!f->factory) return -1;
	memcpy(f->factory, data, size); memcpy(f->data, data, size); f->factory_size = size;
	return 0;
}
static int y2_fs_find(struct y2_store *s, unsigned h, unsigned maximum,
		      unsigned char *entry, unsigned char *name, unsigned *name_size)
{
	struct y2_handle *handle = &s->handles[h];
	memset(entry, 0, 52); memset(name, 0, 256); *name_size = 2;
	for (; handle->next < Y2_FS_FILES;) {
		struct y2_file *f = &s->files[handle->next++];
		unsigned attr = f->directory ? 0x10 : 0x20;
		if (!f->name[0] || fnmatch(handle->pattern, f->name, FNM_PATHNAME) ||
		    (attr & handle->mask) != (handle->attr & handle->mask)) continue;
		const char *base = strrchr(f->name, '/') + 1;
		unsigned n = strlen(base);
		if (!n) continue;
		if (n >= maximum || n >= 128) return -17;
		memset(entry, ' ', 11);
		const char *dot = strrchr(base, '.'); unsigned short_n = dot ? (unsigned)(dot-base) : n;
		memcpy(entry, base, short_n < 8 ? short_n : 8);
		if (dot) { unsigned ext = strlen(dot+1); memcpy(entry+8, dot+1, ext < 3 ? ext : 3); }
		entry[11] = attr; y2_conn_put32(entry+28, f->size);
		for (unsigned j = 0; j < n; j++) name[2*j] = base[j];
		*name_size = (n + 1)*2; return h;
	}
	return -6;
}
static int y2_fs_dispatch(struct y2_store *s, const unsigned char *input, unsigned size,
			  unsigned char *output)
{
	struct y2_fs_packet p; struct y2_reply reply = {output,8,0};
	unsigned a = 0, b = 0, n = 0, h = 0; int result = -2;
	char name[132], other[132]; struct y2_file *f;
	if (y2_fs_parse(input, size, &p) || p.op < 0x1001 || p.op > 0x1021) return -1;
	memset(output, 0, Y2_FS_STRIDE); y2_conn_put32(output, p.op | 0xffff0000U);
	switch (p.op) {
	case 0x1001: case 0x1011: /* OPEN / OPENHINT: hints never bypass checks. */
		if (p.count >= 2 && !y2_fs_name(&p,0,name,0) && !y2_fs_word(&p,1,&a))
			result = y2_fs_open(s,name,a);
		y2_fs_int(&reply,result);
		if (p.op == 0x1011) { unsigned char hint[8] = {0}; y2_fs_arg(&reply,hint,8); }
		break;
	case 0x1002: /* SEEK: signed offset, returned absolute position. */
		if (p.count == 3 && !y2_fs_word(&p,0,&h) && !y2_fs_word(&p,1,&a) &&
		    !y2_fs_word(&p,2,&b) && h < Y2_FS_HANDLES && s->handles[h].file && b < 3) {
			struct y2_handle *v = &s->handles[h];
			int64_t position = (b == 0 ? 0 : b == 1 ? v->position : v->file->size) + (int64_t)(int32_t)a;
			if (position < 0 || position > Y2_FS_FILE_LIMIT) result = -15;
			else result = v->position = position;
		}
		y2_fs_int(&reply,result); break;
	case 0x1003: case 0x1004: /* READ / WRITE */
		if (!y2_fs_word(&p,0,&h) && h < Y2_FS_HANDLES && s->handles[h].file &&
		    !y2_fs_word(&p,p.op == 0x1003 ? 1 : 2,&a) && a <= Y2_FS_STRIDE - 32 &&
		    p.count == (p.op == 0x1003 ? 2U : 3U)) {
			struct y2_handle *v = &s->handles[h]; f = v->file; result = 0;
			if (p.op == 0x1003) {
				n = v->position < f->size ? f->size - v->position : 0;
				if (n > a) n = a;
			} else if (v->flags & 0x100) result = -16;
			else if (p.args[1].size != a) result = -2;
			else if (v->position > Y2_FS_FILE_LIMIT - a) result = -38;
			else {
				if (v->position + a > f->size) result = y2_fs_resize(s,f,v->position+a);
				if (!result) { if (a) memcpy(f->data+v->position,p.args[1].data,a); n = a;
					s->mutations++; if (f->factory) s->shadow_writes++; }
			}
			y2_fs_int(&reply,result); y2_fs_int(&reply,n);
			if (p.op == 0x1003) y2_fs_arg(&reply,n ? f->data+v->position : NULL,n);
			v->position += n;
		} else {
			y2_fs_int(&reply,-2); y2_fs_int(&reply,0);
			if (p.op == 0x1003) y2_fs_arg(&reply,NULL,0);
		}
		break;
	case 0x1005: case 0x1014:
		if (p.count == 1 && !y2_fs_word(&p,0,&h) && h < Y2_FS_HANDLES &&
		    (s->handles[h].file || s->handles[h].search)) { y2_fs_close(s,h); result = 0; }
		y2_fs_int(&reply,result); break;
	case 0x1006: case 0x1017:
		for (h = 0; h < Y2_FS_HANDLES; h++) y2_fs_close(s,h);
		y2_fs_int(&reply,0); break;
	case 0x1007:
		if (p.count == 1 && !(result=y2_fs_name(&p,0,name,0))) {
			if (y2_fs_lookup(s,name)) result = -36;
			else if (!(result=y2_fs_parent(s,name))) result = y2_fs_new(s,name,1) ? 0 : -22;
		}
		y2_fs_int(&reply,result); break;
	case 0x1008: case 0x100f:
		if (p.count == 1 && !(result=y2_fs_name(&p,0,name,0))) {
			f = y2_fs_lookup(s,name); result = -9;
			if (f) {
				result = -16;
				if (!f->factory && !f->refs && strlen(name) > 2 &&
				    f->directory == (p.op == 0x1008)) {
					result = 0;
					for (unsigned i=0;i<Y2_FS_FILES;i++) if (!strncmp(s->files[i].name,name,strlen(name)) &&
					    s->files[i].name[strlen(name)]=='/') result = -16;
					if (!result) { y2_fs_resize(s,f,0); f->name[0]=0; s->mutations++; }
				}
			}
		}
		y2_fs_int(&reply,result); break;
	case 0x1009:
		if (p.count == 1 && !y2_fs_word(&p,0,&h) && h < Y2_FS_HANDLES && s->handles[h].file)
			{ result=0; n=s->handles[h].file->size; }
		y2_fs_int(&reply,result); y2_fs_int(&reply,n); break;
	case 0x100a: case 0x100d:
		if (p.count >= 1 && !y2_fs_name(&p,0,name,0)) {
			f = y2_fs_lookup(s,name); result = f && f->directory ? 0 : -19;
			for (unsigned i=0; !result && i<Y2_FS_FILES; i++) {
				struct y2_file *v = &s->files[i];
				if (!strncmp(v->name,name,strlen(name)) && v->name[strlen(name)]=='/')
					n += p.op == 0x100a ? v->size : 1;
			}
			if (!result) result = n;
		}
		y2_fs_int(&reply,result); break;
	case 0x100b: case 0x100c:
		if (p.count >= 2 && !(result=y2_fs_name(&p,0,name,0)) && !(result=y2_fs_name(&p,1,other,0))) {
			f = y2_fs_lookup(s,name); result = -9;
			if (f && !f->directory && !f->factory && !y2_fs_parent(s,other)) {
				if (y2_fs_lookup(s,other)) result = -36;
				else if (p.op == 0x100b) { strcpy(f->name,other); result = 0; }
				else if (!y2_fs_word(&p,2,&a) && a == 1) {
					struct y2_file *v=y2_fs_new(s,other,0);
					result=v ? y2_fs_resize(s,v,f->size) : -22;
					if (!result && f->size) memcpy(v->data,f->data,f->size);
				} else if (a == 2) { strcpy(f->name,other); result=0; }
				else result = -12;
			}
		}
		y2_fs_int(&reply,result); break;
	case 0x100e: {
		unsigned char disk[84]={0}; memcpy(disk,"Y2 calibration RAM",18); disk[24]='Z';
		y2_conn_put32(disk+36,32); y2_conn_put32(disk+40,1);
		y2_conn_put32(disk+48,512); y2_conn_put32(disk+52,8);
		y2_conn_put32(disk+56,Y2_FS_QUOTA/4096);
		y2_conn_put32(disk+64,(Y2_FS_QUOTA-s->used)/4096);
		y2_fs_int(&reply,0); y2_fs_arg(&reply,disk,sizeof(disk)); break;
	}
	case 0x1010:
		if (p.count == 1 && !(result=y2_fs_name(&p,0,name,0))) {
			f=y2_fs_lookup(s,name); result=f ? (f->directory ? 0x10 : 0x20) : -9;
		}
		y2_fs_int(&reply,result); break;
	case 0x1012: case 0x1013: {
		unsigned char entry[52]={0}, filename[256]={0}; unsigned length=2;
		if (p.op == 0x1012 && p.count == 4 && !y2_fs_name(&p,0,name,1) &&
		    p.args[1].size == 1 && p.args[2].size == 1 && !y2_fs_word(&p,3,&n)) {
			result=y2_fs_handle(s);
			if (result > 0) {
				h=result; struct y2_handle *v=&s->handles[h]; v->search=1;
				strcpy(v->pattern,name); v->attr=p.args[1].data[0]; v->mask=p.args[2].data[0];
				result=y2_fs_find(s,h,n,entry,filename,&length);
				if (result < 0) y2_fs_close(s,h);
			}
		} else if (p.op == 0x1013 && p.count == 2 && !y2_fs_word(&p,0,&h) &&
		    !y2_fs_word(&p,1,&n) && h < Y2_FS_HANDLES && s->handles[h].search)
			result=y2_fs_find(s,h,n,entry,filename,&length);
		y2_fs_int(&reply,result); y2_fs_arg(&reply,entry,52); y2_fs_arg(&reply,filename,length); break;
	}
	case 0x1015: case 0x1016: case 0x1019: case 0x101c:
		/* Single serialized in-memory owner: no host FAT/partition flags. */
		y2_fs_int(&reply,0); break;
	case 0x101a:
		if (p.count == 3 && !y2_fs_word(&p,0,&a) && a == 4) result='Z';
		y2_fs_int(&reply,result); break;
	case 0x101b: y2_fs_int(&reply,4096); break;
	case 0x1021:
		if (p.count == 1 && !(result=y2_fs_name(&p,0,name,0))) {
			f=y2_fs_lookup(s,name); result=-9;
			if (f && f->factory) {
				result=y2_fs_resize(s,f,f->factory_size);
				if (!result) memcpy(f->data,f->factory,f->size);
			}
		}
		y2_fs_int(&reply,result); break;
	default: y2_fs_int(&reply,-12); /* OTP / recursive erase not implemented. */
	}
	return reply.size;
}
static void y2_fs_clear(struct y2_store *s)
{
	for (unsigned i=0;i<Y2_FS_FILES;i++) {
		struct y2_file *f=&s->files[i];
		if (f->data) { memset(f->data,0,f->size); free(f->data); }
		if (f->factory) { memset(f->factory,0,f->factory_size); free(f->factory); }
	}
	memset(s,0,sizeof(*s));
}
#endif
