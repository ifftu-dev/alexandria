#!/usr/bin/env python3
"""Rebuild original bundled teaching clips using macOS speech and FFmpeg."""
import json
from pathlib import Path
import subprocess
import tempfile
import textwrap
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
CONTENT = ROOT / 'demo-world/content'
OUTPUT = CONTENT / 'videos'
FONT = '/System/Library/Fonts/Supplemental/Arial.ttf'

def run(args):
    subprocess.run(args, check=True, stdout=subprocess.DEVNULL)


def main():
    data = json.loads((CONTENT / 'resources.json').read_text())
    OUTPUT.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='alexandria-video-') as tmp:
        work = Path(tmp)
        for item in data['media']:
            parts = []
            for n, scene in enumerate(item['scenes']):
                base = work / f'{item["id"]}-{n}'
                audio = base.with_suffix('.aiff')
                script = base.with_suffix('.speech.txt')
                script.write_text(scene['title'] + '. ' + scene['body'])
                run(['say', '-v', 'Samantha', '-r', '160', '-f', str(script), '-o', str(audio)])
                seconds = float(subprocess.check_output(['ffprobe', '-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', str(audio)])) + 0.8
                title = base.with_suffix('.title.txt')
                body = base.with_suffix('.body.txt')
                title.write_text(textwrap.fill(scene['title'], 40))
                body.write_text(textwrap.fill(scene['body'], 60))
                slide = base.with_suffix('.png')
                canvas = Image.new('RGB', (1280, 720), '#0f172a')
                draw = ImageDraw.Draw(canvas)
                def label(x, y, text, size, color):
                    draw.multiline_text((x, y), text, font=ImageFont.truetype(FONT, size), fill=color, spacing=14)
                draw.rectangle((64,110,164,116), fill='#38bdf8')
                label(64,64,'ALEXANDRIA  /  LEARNING EXAMPLES',20,'#94a3b8')
                label(64,155,title.read_text(),46,'white')
                label(64,300,body.read_text(),34,'#cbd5e1')
                label(64,665,f"AI-generated example  |  Synthetic narration  |  {n+1} / {len(item['scenes'])}",18,'#94a3b8')
                canvas.save(slide)
                part = base.with_suffix('.mp4')
                run(['ffmpeg','-hide_banner','-loglevel','error','-y','-loop','1','-framerate','12','-i',str(slide),'-i',str(audio),'-af','apad','-t',str(seconds),'-c:v','libx264','-preset','veryfast','-crf','24','-threads','2','-pix_fmt','yuv420p','-c:a','aac','-b:a','64k','-ar','44100','-movflags','+faststart',str(part)])
                parts.append(part)
            concat = work / 'parts.txt'
            concat.write_text(''.join(f"file '{p}'\n" for p in parts))
            output = OUTPUT / f'{item["id"]}.mp4'
            run(['ffmpeg','-hide_banner','-loglevel','error','-y','-f','concat','-safe','0','-i',str(concat),'-c','copy','-movflags','+faststart',str(output)])
            item['duration_seconds'] = round(float(subprocess.check_output(['ffprobe','-v','error','-show_entries','format=duration','-of','csv=p=0',str(output)])))
            print(item['id'], item['duration_seconds'], flush=True)
    (CONTENT / 'resources.json').write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')

if __name__ == '__main__':
    main()
