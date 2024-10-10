/*
 * To build this, first build a copy of pkgtools/pkg_install, and then add the
 * paths to dewey.o, opattern.o, and xwrapper.o from that build to the compile
 * line, for example:
 *
 *   $ pinstdir=/path/to/pkgsrc/pkgtools/pkg_install
 *   $ (cd ${pinstdir} && bmake)
 *   $ objdir=$(cd ${pinstdir} && bmake -v WRKSRC)/lib
 *   $ gcc examples/test-pkgmatch.c -o ~/test-pkgmatch \
 *     ${objdir}/dewey.o ${objdir}/opattern.o ${objdir}/xwrapper.o
 *
 * To generate pkgdeps.txt and pkgnames.txt:
 *
 *   $ sqlite3 /var/db/pkgin/pkgin.db \
 *     'SELECT pattern FROM remote_depends' \
 *     | sort | uniq > pkgdeps.txt
 *
 *   $ sqlite3 /var/db/pkgin/pkgin.db \
 *     'SELECT fullpkgname FROM remote_pkg' \
 *     >pkgnames.txt
 *
 * Test files are provided in tests/data.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

extern int pkg_match(const char *, const char *);
extern void *xrealloc(void *, size_t); 
extern void *xmalloc(size_t);

int
main(int argc, char *argv[])
{
	FILE *dfd, *pfd;
	char *dep, *pkg;
	char **deps, **pkgs;
	int d, p;
	size_t dlen, dsize, plen, psize;

	/* Initial size of pkgdeps/pkgnames, tune as necessary */
	dsize = psize = 32768;

	if (argc != 3) {
		fprintf(stderr, "usage: test-pkgmatch <pkgdeps.txt> <pkgnames.txt>");
		exit(1);
	}

	dfd = fopen(argv[1], "r");
	pfd = fopen(argv[2], "r");
	if (dfd == NULL || pfd == NULL)
		exit(1);

	/* Read pkgdeps.txt into **deps */
	d = 0;
	deps = xmalloc(dsize * sizeof(char *));
	while ((dep = fgetln(dfd, &dlen)) != NULL) {
		if (dep[dlen - 1] == '\n')
			dep[dlen - 1] = '\0';
		deps[d++] = strdup(dep);
		if (d == dsize) {
			dsize *= 2;
			deps = xrealloc(deps, dsize * sizeof(char *));
		}

	}
	deps[d] = NULL;

	/* Read pkgnames.txt into **pkgs */
	p = 0;
	pkgs = xmalloc(psize * sizeof(char *));
	while ((pkg = fgetln(pfd, &plen)) != NULL) {
		if (pkg[plen - 1] == '\n')
			pkg[plen - 1] = '\0';
		pkgs[p++] = strdup(pkg);
		if (p == psize) {
			psize *= 2;
			pkgs = xrealloc(pkgs, psize * sizeof(char *));
		}
	}
	pkgs[p] = NULL;

	fclose(dfd);
	fclose(pfd);

	/* Find and print matches */
	d = 0;
	while (deps[d] != NULL) {
		p = 0;
		while (pkgs[p] != NULL) {
			if (pkg_match(deps[d], pkgs[p])) {
				printf("%s %s\n", deps[d], pkgs[p]);
			}
			p++;
		}
		d++;
	}
}
