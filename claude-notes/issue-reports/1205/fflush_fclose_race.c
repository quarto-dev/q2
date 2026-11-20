/* Issue #1205 follow-up: does fflush(NULL) deadlock against plain fopen/fclose
 * (no popen)? Half the threads loop fflush(NULL); half loop fopen+fwrite+fclose.
 * This is what Lua's io.popen (fflush(NULL)) in one state racing io.open/close
 * (or a GC'd file handle) in another would do.
 * Build: cc -O2 -pthread fflush_fclose_race.c -o fflush_fclose_race */
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

static int iters = 20000;
static volatile long done = 0;

static void *flusher(void *a) { (void)a; for (int i = 0; i < iters; i++) { fflush(NULL); __sync_fetch_and_add(&done, 1); } return NULL; }
static void *closer(void *a) {
  (void)a;
  for (int i = 0; i < iters; i++) {
    FILE *f = fopen("/dev/null", "w");
    if (!f) { perror("fopen"); exit(1); }
    fputs("x", f);
    fclose(f);
    __sync_fetch_and_add(&done, 1);
  }
  return NULL;
}

int main(int argc, char **argv) {
  int n = argc > 1 ? atoi(argv[1]) : 12;
  if (argc > 2) iters = atoi(argv[2]);
  pthread_t *t = calloc(n, sizeof *t);
  for (int i = 0; i < n; i++) pthread_create(&t[i], NULL, i % 2 ? closer : flusher, NULL);
  long last = -1; int stalls = 0;
  while (done < (long)n * iters) {
    sleep(1);
    long d = done;
    if (d == last && ++stalls >= 5) { fprintf(stderr, "STALLED at %ld/%ld\n", d, (long)n * iters); return 2; }
    if (d != last) stalls = 0;
    last = d;
  }
  fprintf(stderr, "OK\n");
  return 0;
}
