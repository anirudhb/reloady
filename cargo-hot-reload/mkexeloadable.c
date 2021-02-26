/** Credit: https://gist.github.com/xieyubo/6820491646f9d01980a7eadb16564ddf **/
/*
There is a behavior change in glibc>=2.30. dlopen() will fail if the file is
position independent executable. This code will clean up the PIE flag in the
file to bypass the dlopen() check.

   MIT License (c) 2020 Yubo Xie, xyb@xyb.name
*/

#include <elf.h>
#include <stdio.h>
#include <string.h>

#define EXEC(x, err_msg) \
  if (!(x)) {            \
    perror(err_msg);     \
    err = -1;            \
    goto ex;             \
  }

int main(int argc, char* argv[]) {
  int err = 0;

  if (argc != 2) {
    printf("Usage: mkexeloadable <executable-elf64-file>\n");
    return -1;
  }

  FILE* fp = fopen(argv[1], "r+b");
  EXEC(fp != NULL, "Can't open file");

  // Read elf header.
  Elf64_Ehdr elf_header;
  EXEC(fread(&elf_header, sizeof(elf_header), 1, fp) == 1,
       "Read elf64_ehdr failed");

  // Is it a elf64 file?
  EXEC(strncmp((char*)elf_header.e_ident, "\177ELF\002", 5) == 0,
       "It is not a valid elf64 file");

  // Find out dynamic section
  Elf64_Shdr section_header;
  Elf64_Dyn dyn;
  EXEC(fseek(fp, elf_header.e_shoff, SEEK_SET) == 0, "Seek file failed");
  for (int i = 0; i < elf_header.e_shnum; ++i) {
    EXEC(fread(&section_header, elf_header.e_shentsize, 1, fp) == 1,
         "Read section header failed");
    if (section_header.sh_type == SHT_DYNAMIC) {
      EXEC(fseek(fp, section_header.sh_offset, SEEK_SET) == 0,
           "Seek file failed");
      while (1) {
        EXEC(fread(&dyn, sizeof(Elf64_Dyn), 1, fp) == 1,
             "Read section entry failed");
        EXEC(dyn.d_tag != 0, "Can't find flags_1");
        if (dyn.d_tag == 0x6ffffffb) {
          // Clear PIE flag.
          dyn.d_un.d_val &= ~DF_1_PIE;
          EXEC(fseek(fp, -sizeof(Elf64_Dyn), SEEK_CUR) == 0,
               "Seek flags_1 failed");
          EXEC(fwrite(&dyn, sizeof(Elf64_Dyn), 1, fp) == 1,
               "Write back flags_1 failed");
          // Success!
          goto ex;
        }
      }
    }
  }

  // Can't find required flags, something might be wrong.
  EXEC(0, "Can't find required flags");

ex:
  if (fp) {
    fclose(fp);
  }
  return err;
}