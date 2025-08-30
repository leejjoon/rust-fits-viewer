Here is a formal Product Requirements Document (PRD) for the proposed FITS viewer.

# Product Requirements Document: Interactive FITS Viewer for Jupyter

**Author:** Award-Winning Writer
**Version:** 1.0
**Date:** July 16, 2025

## 1\. Introduction

### 1.1 Problem Statement

The scientific community, particularly in astronomy, relies heavily on the FITS format for image data. While a variety of FITS viewers exist, there is a significant gap in the tooling available for modern, remote-first analysis workflows. Researchers using the Jupyter ecosystem on remote servers face a major bottleneck: the need to transfer massive FITS files to the local machine for visualization, which is inefficient and slow. Existing tools are either architecturally outdated (DS9), not designed for a client-server model (Ginga), or are heavyweight enterprise solutions with significant deployment overhead (Firefly).

### 1.2 Proposed Solution

This document outlines the requirements for a new, open-source FITS image viewer designed specifically to solve this problem. The product will be a lightweight, high-performance, client-server application that integrates seamlessly into the Jupyter environment. It will keep large data on the server and only transmit the necessary pixels for visualization to a browser-based client, enabling fluid, interactive exploration of massive datasets directly within a notebook.

## 2\. Goals and Objectives

  * **Primary Goal:** To create the most performant and user-friendly solution for interactively viewing large FITS files and in-memory NumPy arrays within a remote Jupyter environment.
  * **Key Objectives:**
      * **Eliminate Data Transfer Bottlenecks:** Implement a client-server architecture where the server handles all large file I/O and processing.
      * **Ensure High Performance:** Leverage Rust, WebAssembly, and GPU acceleration (`wgpu`) to deliver a smooth, responsive, real-time user experience (panning, zooming, adjustments).
      * **Achieve Seamless Jupyter Integration:** Provide a powerful, bidirectional connection between the visual tool and the Python kernel using `ipywidgets`.
      * **Promote Accessibility and Adoption:** Ensure the tool is easy to install (`pip`), deploy (Docker), and use, with a permissive open-source license to encourage community contribution.

## 3\. Target Audience

  * **Primary Persona: The Remote Astronomer.** A researcher who connects to a university or data center's JupyterHub instance to analyze large FITS files (multi-gigabyte mosaics, data cubes) stored on a remote server. They need to quickly inspect images, identify features, and link visual information back to their Python analysis code without downloading files.
  * **Secondary Persona: The Computational Data Scientist.** A researcher in a field like medical imaging or materials science who generates large, high-dynamic-range image arrays (e.g., NumPy arrays) through computation. They need to visualize the results of their algorithms immediately and interactively within their notebook to iterate on their work quickly.

## 4\. Functional Requirements & Features

This section details the features of the product, prioritized into Must-Haves (for the Minimum Viable Product), Should-Haves (for a full v1.0 release), and Nice-to-Haves (for future versions).

| Priority | Feature ID | Feature Description | User Story |
| :--- | :--- | :--- | :--- |
| **P0 (Must-Have)** | **CORE-01** | **Client-Server Architecture** | As a user, I want the viewer to operate in a client-server model so that large FITS files remain on the server, minimizing network latency. |
| **P0 (Must-Have)** | **CORE-02** | **Dynamic Tiling & Downsampling** | As a user, I want the server to dynamically send only the visible portion of an image (as tiles) at the appropriate resolution, so I can view massive images smoothly. |
| **P0 (Must-Have)** | **VIEW-01** | **Interactive Pan & Zoom** | As a user, I want to pan and zoom interactively using my mouse (drag to pan, scroll to zoom) to explore the image. |
| **P0 (Must-Have)** | **JUP-01** | **Basic Jupyter Widget** | As a user, I want to instantiate the viewer in a notebook cell by calling a simple Python function with a file path (e.g., `FITSViewer('path/to/image.fits')`). |
| **P0 (Must-Have)** | **IMG-01** | **Basic Image Stretching** | As a user, I want to apply basic contrast stretches (Linear, Log, Square Root) to see features in high-dynamic-range data. |
| **P1 (Should-Have)** | **DATA-01** | **In-Memory NumPy Array Viewing** | As a data scientist, I want to pass a NumPy array directly to the viewer (e.g., `FITSViewer(my_array)`) to visualize my algorithm's output without saving it to a file. |
| **P1 (Should-Have)** | **JUP-02** | **Bidirectional State Sync** | As a user, I want the Python kernel to be aware of my interactions (e.g., `viewer.zoom` reflects the current zoom level) and be able to programmatically control the view (e.g., setting `viewer.center_x` updates the display). |
| **P1 (Should-Have)** | **ASTRO-01** | **WCS Coordinate Display** | As an astronomer, I want to see the World Coordinate System (WCS) coordinates (e.g., RA/Dec) of my cursor as I move it over the image. |
| **P1 (Should-Have)** | **ASTRO-02** | **FITS Header Display** | As an astronomer, I want to be able to view the FITS header of the image within the widget interface. |
| **P1 (Should-Have)** | **UI-01** | **Interactive UI Controls** | As a user, I want clear UI controls (e.g., dropdowns, sliders) within the widget to change settings like the image stretch function. |
| **P2 (Nice-to-Have)** | **EXT-01** | **Extensible Plugin Architecture** | As a developer, I want a clear plugin architecture so I can extend the viewer with new tools and functionality. |
| **P2 (Nice-to-Have)** | **OVERLAY-01**| **Catalog Marker Overlay** | As a user, I want to overlay a list of source positions (e.g., from a catalog) as markers on the image. |

## 5\. Non-Functional Requirements

| Category | Requirement |
| :--- | :--- |
| **Performance** | - **Responsiveness:** Panning and zooming must be fluid, maintaining a target of \>30 FPS on a standard laptop. \<br\> - **Load Time:** Initial view and subsequent tile loads should feel near-instantaneous over a typical institutional network connection. |
| **Scalability** | - **Image Size:** The viewer must handle FITS images of at least 30,000 x 30,000 pixels without performance degradation. \<br\> - **Concurrency:** The server should be able to handle requests from multiple simultaneous users without significant slowdown. |
| **Usability** | - **Installation:** The Python package must be installable via a single `pip install` command. \<br\> - **Ease of Use:** The Python API should be simple and intuitive for users familiar with the scientific Python ecosystem. |
| **Compatibility** | - **Client:** The WASM client must function correctly on the latest versions of major web browsers (Chrome, Firefox, Safari) that support WebGL2. \<br\> - **Server:** The server must be deployable on modern Linux distributions. |
| **Deployment** | - **Containerization:** A Docker image for the Rust server must be provided for easy and consistent deployment in cloud or on-premise environments. \<br\> - **Packaging:** The Python component must be published to the Python Package Index (PyPI). |
| **Documentation** | - **User Guide:** Clear documentation on how to install, use, and configure the viewer. \<br\> - **Developer Guide:** Architectural overview and instructions for contributing to the project. |

## 6\. Out of Scope (for v1.0)

  * Full image editing and manipulation (e.g., cropping, rotating the source data).
  * Visualization of FITS binary or ASCII tables.
  * Advanced catalog interactions (e.g., filtering, searching, cross-matching within the UI).
  * A standalone desktop application (the focus is exclusively on the Jupyter environment).
  * Support for proprietary, non-FITS image formats.

## 7\. Success Metrics

  * **Adoption:**
      * Number of PyPI downloads per month.
      * Number of stars and forks on the GitHub repository.
  * **Performance:**
      * Benchmark comparisons showing significantly faster time-to-view for large remote files compared to Ginga-in-Jupyter.
      * Achieving and maintaining \>30 FPS during interactive use.
  * **Community Engagement:**
      * Number of bug reports and feature requests from the community.
      * Number of pull requests from external contributors.
  * **User Satisfaction:**
      * Qualitative feedback from users on community channels (GitHub, forums) indicating the tool solves their remote visualization problems.
