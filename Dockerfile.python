FROM python:3.11-slim-bookworm

# 使用中国的 APT 源（清华镜像），兼容 Debian 12 的 debian.sources 与传统 sources.list
RUN set -eux; \
    if [ -f /etc/apt/sources.list ]; then \
      cp /etc/apt/sources.list /etc/apt/sources.list.bak; \
      sed -i -E \
        -e 's~https?://[^ ]+/debian~https://mirrors.tuna.tsinghua.edu.cn/debian~g' \
        -e 's~https?://security.debian.org/debian-security~https://mirrors.tuna.tsinghua.edu.cn/debian-security~g' \
        /etc/apt/sources.list; \
    fi; \
    if [ -f /etc/apt/sources.list.d/debian.sources ]; then \
      cp /etc/apt/sources.list.d/debian.sources /etc/apt/sources.list.d/debian.sources.bak; \
      sed -i -E \
        -e 's~https?://deb\.debian\.org/debian~https://mirrors.tuna.tsinghua.edu.cn/debian~g' \
        -e 's~https?://security\.debian\.org/debian-security~https://mirrors.tuna.tsinghua.edu.cn/debian-security~g' \
        /etc/apt/sources.list.d/debian.sources; \
    fi; \
    if [ ! -f /etc/apt/sources.list ] && [ ! -f /etc/apt/sources.list.d/debian.sources ]; then \
      printf '%s\n' \
        'deb https://mirrors.tuna.tsinghua.edu.cn/debian/ bookworm main contrib non-free non-free-firmware' \
        'deb https://mirrors.tuna.tsinghua.edu.cn/debian/ bookworm-updates main contrib non-free non-free-firmware' \
        'deb https://mirrors.tuna.tsinghua.edu.cn/debian/ bookworm-backports main contrib non-free non-free-firmware' \
        'deb https://mirrors.tuna.tsinghua.edu.cn/debian-security bookworm-security main contrib non-free non-free-firmware' \
        > /etc/apt/sources.list; \
    fi

# 安装基础依赖（包含 TLS 证书与构建工具，以便 uv/部分轮子工作）
RUN apt-get update && apt-get install -y \
    ca-certificates curl git vim \
    nodejs build-essential pkg-config cmake ninja-build \
    libreoffice pandoc ffmpeg \
    libgl1 libglib2.0-0 \
  && apt-get clean && rm -rf /var/lib/apt/lists/*
  
# 安装常用库
RUN pip install numpy pandas scipy markdown pypandoc langchain langgraph mcp onnx transformers \
    python-dateutil scikit-learn sqlalchemy psycopg[binary] pymysql pymongo openpyxl xlrd xlwt xlsxwriter PyYAML fastmcp \
    reportlab pyarrow matplotlib seaborn weasyprint fastapi uvicorn starlette sse-starlette pydantic \
    jinja2 jupyterlab flask flask-restx requests aiohttp httpx scrapy -i https://pypi.tuna.tsinghua.edu.cn/simple

RUN pip install bcrypt pyjwt python-dotenv oauthlib celery redis opencv-python pillow pygame \
    pytest faker coverage pytest-mock python-magic unidecode tqdm loguru rich \
    poetry pipenv beautifulsoup4 typer pywebio python-docx python-pptx PyPDF2 pdf2docx -i https://pypi.tuna.tsinghua.edu.cn/simple

RUN pip install psutil -i https://pypi.tuna.tsinghua.edu.cn/simple

# 安装 Node.js 22.x（LTS）
RUN apt-get update && \
    apt-get install -y ca-certificates curl gnupg && \
    mkdir -p /etc/apt/keyrings && \
    curl -fsSL https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key | gpg --dearmor -o /etc/apt/keyrings/nodesource.gpg && \
    echo "deb [signed-by=/etc/apt/keyrings/nodesource.gpg] https://deb.nodesource.com/node_22.x nodistro main" | tee /etc/apt/sources.list.d/nodesource.list && \
    apt-get update && \
    apt-get install -y nodejs && \
    rm -rf /var/lib/apt/lists/*


# EVA_SKILLS 依赖补充（追加层）
RUN apt-get update && apt-get install -y --no-install-recommends \
    poppler-utils qpdf pdftk tesseract-ocr \
  && apt-get clean && rm -rf /var/lib/apt/lists/*

RUN pip install \
    pypdf pdfplumber pytesseract pdf2image imageio defusedxml playwright \
  -i https://pypi.tuna.tsinghua.edu.cn/simple

RUN python -m playwright install --with-deps chromium \
  && rm -rf /var/lib/apt/lists/*

RUN npm install -g docx pptxgenjs playwright react react-dom react-icons sharp \
  && npx playwright install chromium

RUN pip install \
    pytest-asyncio python-jose passlib python-multipart\
  -i https://pypi.tuna.tsinghua.edu.cn/simple

RUN pip install psycopg -i https://pypi.tuna.tsinghua.edu.cn/simple

# 设置工作目录
WORKDIR /workspaces

CMD ["/bin/bash"]

# docker buildx build --platform linux/arm64 -t wunder:20250105-arm64 -f Dockerfile .
# docker buildx build --platform linux/x86_64 -t wunder:20250105-x86 -f Dockerfile .

