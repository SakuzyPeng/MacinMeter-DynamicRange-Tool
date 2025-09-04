## Measuring DR

Each channel of the audio signal is splitted into blknum blocks of 3 seconds length. The RMS is calculated for each block as the square root of the double sum over all input samples squared, divided by the block size in samples. The Peak is calculated as the maximum of the absolute value from the block:

$$
{RMS} = \sqrt{2 \cdot  \frac{\mathop{\sum }\limits_{{i = 1}}^{n}{\mathrm{{smp}}}_{i}^{2}}{n}}\;\text{ with }\mathrm{n} = {132480}\text{ for }{44.1}\mathrm{{kHz}}\text{ Samplerate } \tag{1}
$$

$$
\text{Peak} = \mathop{\max }\limits_{{i = 1}}^{n}\left( \left| {\mathrm{{smp}}}_{i}\right| \right) \;\text{with}\mathrm{n} = {132480}\text{for}{44.1}\mathrm{{kHz}}\text{Samplerate} \tag{2}
$$

Each RMS and Peak value is entered into a histogram with 10,000 discrete values ranging from $- {100}\mathrm{\;{dB}}$ to $0\mathrm{\;{dB}}$ in steps of ${0.01}\mathrm{\;{dB}}$ . Conversion of ${Rms}$ and ${Peak}$ into $\mathrm{{dB}}$ is done according to:

$$
\text{out}\left\lbrack  {dB}\right\rbrack   = {20} \cdot  {\log }_{10}\left( \text{in}\right)  \tag{3}
$$

The DR value for each channel $j$ can then be derived as the difference between the second largest Peak value and the RMS-sum over the upper 20% histogram values converted to dB:

$$
D{R}_{j}\left\lbrack  {dB}\right\rbrack   =  - {20} \cdot  {\log }_{10}\left( {\sqrt{\frac{\mathop{\sum }\limits_{{n = 1}}^{N}{RM}{S}_{n}^{2}}{N} \cdot  \frac{1}{P{k}_{2nd}}}\text{ with }N = {0.2} \cdot  \text{ blknum }}\right)  \tag{4}
$$

The overall DR is finally calculated as the average of the channel DR values rounded to the next integer value.

Using the RMS-sum in (4) results in the overall RMS of the upper 20% of the input material, eliminating the contribution of small Peaks. This method also ensures that the resulting DR value is virtually independent from the block size used (3s in this example) as long as this is small compared to the overall input material length.

Limiting the DR-measurement to the upper 20% of the blocks with maximum RMS is a compromise that allows to somewhat compare a wide variety of different material in a quantitative way. Also in highly dynamic Material only the loudest parts, which usually best reflect the processing of the material (compression etc.), contribute to the DR measurement.