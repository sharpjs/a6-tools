#!/usr/bin/env ruby
#
# a6pkg - A6 Software Update Packager/Unpackager
#
# Copyright (C) 2015 Jeffrey Sharp
#
# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program.  If not, see <http://www.gnu.org/licenses/>.

require 'optparse'
require 'pathname'

# os:         os.syx -> os.bin
# boot:     boot.syx -> boot.bin
# nvram: patches.syx -> patches/pgm-01-002.bin

module A6
  class Packager
    attr_reader :options

    def run(args)
      @options = parse_options(args)
      send(@options[:mode] || :decode)
    end

    def parse_options(args)
      {}.tap do |options|
        OptionParser.new do |p|
          p.banner = "usage: a6pkg [options] [file ...]"

          p.on("-d", "--decode", "Convert SysEx to binary (default).") do
            options[:mode] = :decode
          end

          p.on("-o", "--output FILE", "Write output to file.") do |file|
            options[:output] = file
          end

          p.on("-v", "--verbose", "Enable verbose output.") do
            options[:verbose] = true
          end

          p.on("-h", "--help", "Print this help message.") do
            puts p
            exit
          end
        end.parse!(args)
      end
    end

    def decode
      decoder = options[:decoder]

      each_file do |path, bytes|
        bytes.scan(SYSEX) do |opcode, data|
          opcode = opcode.ord
          decoder ||= decoder_for(opcode)
          decoder.source = path
          decoder.pos = $~.begin(0)
          decoder.decode(opcode, data)
        end
      end

      decoder.flush if decoder
    end

    private

    def each_file
      ARGF.binmode
      loop do
        begin
          yield ARGF.filename, ARGF.file.read
        rescue FormatError
          exit 1
        end
        ARGF.skip
        break if ARGV.empty?
      end
    end

    def decoder_for(opcode)
      klass =
        case opcode
        when 0x30 then OsDecoder
        when 0x3F then BootDecoder
        else           NvramDecoder
        end
      klass.new(options)
    end

    # A6 SysEx Frame Format
    SYSEX = /
      \xF0            # SysEx start
      \x00\x00\x0E    # Manufacturer ID
      \x1D            # Family ID
      ([\x00-\x7F])   # Opcode
      ([\x00-\x7F]*+) # Data
      \xF7            # SysEx end
    /xn
  end

  class FormatError < StandardError
  end

  class Decoder
    attr_accessor :source, :pos
    attr_reader   :target, :options

    def initialize(options)
      @options = options
    end

    def source=(name)
      if name && name != '-'
        @source   = name.to_s
        @target ||= target_for(@source)
      else
        @source   = "(stdin)"
        @target ||= target_for(nil)
      end
      info "scanning #{name}"
    end

    def decode(opcode, data)
      # Nothing
    end

    def flush
      # Nothing
    end

    protected

    def decode_7bit(midi)
      "".b.tap do |data|
        offset = 0
        bits8  = 0
        midi.each_byte do |bits7|
          n = offset % 8
          offset += 1
          if n == 0
            bits8 = bits7
          else
            bits8 |= bits7 << (8 - n)
            data << (bits8 & 0xFF)
            bits8 >>= 8
          end
        end
      end
    end

    def target_for(src)
      tgt = options[:output] and Pathname.new(tgt)
    end

    def should_write?(file)
      !file.exist? || !options[:overwrite] or
        warning "file exists; skipping: #{file}"
    end

    def require_value(name, value, expected, format = '%s')
      unless expected === value
        value    = format % value
        expected = format % expected
        error "expected #{name} #{expected}, but found #{value}"
      end
    end

    def require_range(name, value, range, format = '%s')
      unless range.include?(value)
        value = format % value
        min   = format % range.min
        max   = format % range.max
        error "expected #{name} in range #{min}..#{max}, but found #{value}"
      end
    end

    def info(str)
      $stderr.puts message(str) if options[:verbose]
    end

    def warning(str)
      $stderr.puts message(str, :warning)
    end

    def error(str)
      $stderr.puts message(str, :error)
      raise FormatError
    end

    def message(str, level = nil)
      "".tap do |s|
        s << source
        s << '['  << pos.to_s << ']' if pos
        s << ': ' << level.to_s      if level
        s << ': ' << str
      end
    end
  end

  class BlockDecoder
    attr_reader :version, :checksum, :length, :blocks

    TARGET_EXT = ".bin"

    def initialize(options)
      super options
      @blocks = []
      @sum    = 0
    end

    def decode(opcode, data)
      # Unpack SysEx frame
      require_value "sysex opcode", opcode,      self.opcode, '%02Xh'
      require_value "block of",     data.length, BLOCK_LEN,   '%d bytes'
      data = decode_7bit(data)

      # Read header
      version, checksum, length, block_count, block_index =
        data.unpack('L>3S>2')

      if !@version
        # Capture header values from first block
        @version, @checksum, @length, @block_count, @blocks =
         version,  checksum,  length,  block_count, Array.new(block_count)
      else
        # Validate header
        require_value 'version',     version,     @version,  '%04Xh'
        require_value 'checksum',    checksum,    @checksum, '%04Xh'
        require_value 'length',      length,      @length
        require_value 'block count', block_count, @block_count
      end

      # Validate block index
      require_range 'block index', block_index, 0...block_count
      warning "duplicate block #{block_index}" if @blocks[block_index]

      # Extract data
      length = [[@length - @offset, DATA_LEN].min, 0].max
      data   = data[HEADER_LEN, length]
      sum    = data.each_byte.reduce(:+)

      # Store data
      @blocks[block_index] = data
      @sum = (@sum + sum) & 0xFFFFFFFF
      self
    end

    def flush
      pos = nil

      nil_index = @blocks.find_index(nil) and
        error "one or more blocks missing, starting at block #{nil_index}"

      @checksum == @sum or
        error "calculated sum #{'%08Xh' % @sum} does not match "\
            "header checksum #{'%08Xh' % @checksum}"

      should_write?(target) and 
        p "eh" # target.open("wb") { |f| @blocks.reduce(f, :<<) }
    end

    private

    HEADER_LEN =  16 # bytes
    DATA_LEN   = 256 # bytes
    BLOCK_LEN  = 311 # bytes, packed 8-to-7 bits

    def write_metadata_to(io)
      io.puts \
        "Version:  #{format_version(version)}",
        "Checksum: #{'%08X' % checksum}",
        "Length:   #{'%8d'  % length} bytes",
        "Blocks:   #{'%8d'  % blocks.count} 256-byte blocks"
      self
    end

    def format_version(v)
      "#{v / 10000}.#{v / 100 % 100}.#{v % 100}"
    end
  end

  class OsDecoder < BlockDecoder
    def opcode
      0x30
    end

    def target_for(src)
      super(src) || Pathname.new(src || "a6-os").sub_ext(".bin")
    end
  end

  class BootDecoder < BlockDecoder
    def opcode
      0x3F
    end

    def target_for(src)
      super(src) || Pathname.new(src || "a6-boot").sub_ext(".bin")
    end
  end

  class NvramDecoder < Decoder
    TARGET_EXT = ".bin"

    def decode(opcode, data)
      handler = OPCODES[opcode] or
        warning "unsupported opcode: #{'%02Xh' % opcode}"
      send handler, data
    end

    def target_for(src)
      super(src) || Pathname.new(src || "a6-data").sub_ext("")
    end

    private

    OPCODES = {
      0x00 => :op_program
    }

    PROGRAM_LEN = 2343

    def op_program(data)
      require_value "program dump of", data.length, PROGRAM_LEN

      bank, program = data.unpack('CC')
      require_range "bank#",    bank,    0.. 15
      require_range "program#", program, 0..127

      data = decode_7bit(data[2..-1])
      file = target + 'pgm-%02d-%03d.bin' % [bank, program]

      info "found bank #{bank}, program #{program}"

      target.mkdir unless target.directory?

      should_write?(file) and
        file.binwrite data
    end
  end
end

if __FILE__ == $0
  # Running as a script
  A6::Packager.new.run(ARGV)
end

# vim: sw=2 sts=2
