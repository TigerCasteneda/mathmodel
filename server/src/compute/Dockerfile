FROM python:3.11-slim

RUN pip install --no-cache-dir \
    numpy \
    scipy \
    matplotlib \
    pandas \
    sympy

RUN mkdir -p /tmp/output
WORKDIR /tmp
CMD ["python"]
