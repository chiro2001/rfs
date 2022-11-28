line = "%015x\n"
line_count = 40 * 0x400 * 0x400 // 16
filename = "textfile.txt"
with open(filename, "w", encoding="utf8") as f:
    f.writelines([line % (i * 16) for i in range(line_count)])