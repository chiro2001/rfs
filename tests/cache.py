import os
import sys
import time

mnt_point = '/home/chiro/mnt'
f_path = mnt_point + '/file'


def process(loop, cblks):
    print(f"Cache Blks: {cblks}")
    work_set_sz = 16 * 512  # 8KB
    iter_sz = 512  # 512B
    num_iters = work_set_sz // iter_sz
    tot_sz = loop * work_set_sz  # B
    start = time.time()
    if cblks != 0:
        os.system(f'cargo run --release -- --format -q -c --cache_size {cblks} {mnt_point}')
    else:
        os.system(f'cargo run --release -- --format -q {mnt_point}')
    # print("started")
    time.sleep(1)
    with open(f_path, 'w+') as f:
        content = 'a' * work_set_sz
        for i in range(loop):
            f.seek(0)
            for j in range(num_iters):
                # print("Iter: " + str(j))
                f.write(content[j * iter_sz: (j + 1) * iter_sz])

        f.seek(0)
        data = f.read(work_set_sz)
        if data != content:
            print('buf: read data is not equal to written data')
            sys.exit(1)

    os.system('umount {}'.format(mnt_point))
    end = time.time()
    print('Time: {}ms BW: {}MB/s'.format(1000 * (end - start), tot_sz / 1024 / 1024 / (end - start)))


loop = 1000000
# loop = 100000
process(loop, 512)
process(loop, 0)
