#include <errno.h>
#include <signal.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

extern char **environ;

static volatile pid_t child = 0;

static void forward(int sig) {
    if (child > 0) kill(child, sig);
}

int main(int argc, char **argv) {
    const char *exit_file = NULL, *pid_file = NULL, *cwd = NULL;
    int i = 1;
    for (; i < argc; i++) {
        if (!strcmp(argv[i], "--exit-file") && i + 1 < argc) exit_file = argv[++i];
        else if (!strcmp(argv[i], "--pid-file") && i + 1 < argc) pid_file = argv[++i];
        else if (!strcmp(argv[i], "--cwd") && i + 1 < argc) cwd = argv[++i];
        else if (!strcmp(argv[i], "--")) { i++; break; }
        else break;
    }
    if (i >= argc) {
        fprintf(stderr, "usage: sc-dev-host [--exit-file F] [--pid-file F] [--cwd DIR] -- /abs/path/to/bin [args...]\n");
        return 64;
    }
    if (cwd && chdir(cwd) != 0) {
        fprintf(stderr, "sc-dev-host: chdir %s: %s\n", cwd, strerror(errno));
        return 66;
    }

    struct sigaction sa = {0};
    sa.sa_handler = forward;
    sigaction(SIGTERM, &sa, NULL);
    sigaction(SIGINT, &sa, NULL);
    sigaction(SIGHUP, &sa, NULL);

    pid_t pid;
    int rc = posix_spawn(&pid, argv[i], NULL, NULL, &argv[i], environ);
    if (rc != 0) {
        fprintf(stderr, "sc-dev-host: spawn %s: %s\n", argv[i], strerror(rc));
        return 127;
    }
    child = pid;
    if (pid_file) {
        FILE *f = fopen(pid_file, "w");
        if (f) { fprintf(f, "%d\n", pid); fclose(f); }
    }

    int status = 0;
    while (waitpid(pid, &status, 0) < 0 && errno == EINTR) {}
    int code = WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
    if (exit_file) {
        FILE *f = fopen(exit_file, "w");
        if (f) { fprintf(f, "%d\n", code); fclose(f); }
    }
    return code;
}
