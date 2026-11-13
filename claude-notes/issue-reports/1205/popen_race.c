/* Issue #1205: does libc deadlock when threads run Lua's l_popen sequence
 * (fflush(NULL); popen; read; pclose) concurrently? No Lua, no q2.
 * Build: cc -O2 -pthread popen_race.c -o popen_race
 * Run:   ./popen_race [threads] [iters] [lock]   ("lock" = serialize with one mutex)
 * Prints a heartbeat every second; a hang shows as a stalled counter. */
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int iters = 200, use_lock = 0;
static volatile long done = 0;
static pthread_mutex_t g = PTHREAD_MUTEX_INITIALIZER;

static void *worker(void *arg) {
  (void)arg;
  char buf[64];
  for (int i = 0; i < iters; i++) {
    if (use_lock) pthread_mutex_lock(&g);
    fflush(NULL);
    FILE *p = popen("echo hi", "r");
    if (use_lock) pthread_mutex_unlock(&g);
    if (!p) { perror("popen"); exit(1); }
    while (fread(buf, 1, sizeof buf, p) > 0) {}
    if (use_lock) pthread_mutex_lock(&g);
    pclose(p);
    if (use_lock) pthread_mutex_unlock(&g);
    __sync_fetch_and_add(&done, 1);
  }
  return NULL;
}

int main(int argc, char **argv) {
  int n = argc > 1 ? atoi(argv[1]) : 12;
  if (argc > 2) iters = atoi(argv[2]);
  if (argc > 3 && strcmp(argv[3], "lock") == 0) use_lock = 1;
  pthread_t *t = calloc(n, sizeof *t);
  for (int i = 0; i < n; i++) pthread_create(&t[i], NULL, worker, NULL);
  long last = -1; int stalls = 0;
  while (done < (long)n * iters) {
    sleep(1);
    long d = done;
    fprintf(stderr, "done=%ld/%ld\n", d, (long)n * iters);
    if (d == last && ++stalls >= 5) { fprintf(stderr, "STALLED\n"); return 2; }
    if (d != last) stalls = 0;
    last = d;
  }
  fprintf(stderr, "OK\n");
  return 0;
}
